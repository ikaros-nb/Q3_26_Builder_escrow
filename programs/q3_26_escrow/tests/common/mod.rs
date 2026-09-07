#![allow(dead_code, unused_imports)]

use anchor_lang::{
    AccountDeserialize, InstructionData, ToAccountMetas,
    prelude::Pubkey,
    solana_program::{clock::Clock, instruction::{Instruction, error::InstructionError}},
    system_program::ID as SYSTEM_PROGRAM_ID,
};
use anchor_spl::associated_token::ID as ASSOCIATED_TOKEN_PROGRAM_ID;
use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata},
    LiteSVM,
};
use litesvm_token::{
    get_spl_account, CreateAssociatedTokenAccount, CreateAssociatedTokenAccountIdempotent,
    CreateMint, MintTo,
    spl_token::{ID as TOKEN_PROGRAM_ID, state::Account as TokenAccount},
};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;

// Re-exported so the test files need only `use common::*`.
pub use anchor_lang::error::ErrorCode as AnchorErrorCode;
pub use anchor_lang::prelude::msg;
pub use anchor_spl::associated_token::get_associated_token_address;
pub use q3_26_escrow::{error::EscrowError, MAX_ESCROW_DURATION};
pub use solana_signer::Signer;

/// Convenience alias for what every instruction helper returns.
pub type TxResult = Result<TransactionMetadata, FailedTransactionMetadata>;

pub const SEED: u64 = 42;
pub const SUPPLY: u64 = 1_000_000_000;
pub const DEPOSIT: u64 = 1_000_000;
pub const RECEIVE: u64 = 1_000_000;
pub const TEN_DAYS: i64 = 864_000; // 10 days in seconds, well under MAX_ESCROW_DURATION (30 days)
// A plausible wall-clock timestamp. LiteSVM starts the Clock sysvar at 0, which would make
// "now - 10 days" a negative unix timestamp — an input no real cluster can produce.
pub const START_TIMESTAMP: i64 = 1_788_683_523;

pub struct Setup {
    pub svm: LiteSVM,
    pub maker: Keypair,
    pub taker: Keypair,
    pub mint_a: Pubkey,
    pub mint_b: Pubkey,
    pub maker_ata_a: Pubkey,
    pub maker_ata_b: Pubkey,
    pub taker_ata_a: Pubkey,
    pub taker_ata_b: Pubkey,
    pub escrow: Pubkey,
    pub vault: Pubkey,
}

impl Setup {
    pub fn new() -> Self {
        let program_id = q3_26_escrow::id();

        let mut svm = LiteSVM::new();
        let bytes = include_bytes!(concat!(
            env!("CARGO_TARGET_TMPDIR"),
            "/../deploy/q3_26_escrow.so"
        ));
        svm.add_program(program_id, bytes)
            .expect("load program: run `anchor build` first");

        let maker = Keypair::new();
        svm.airdrop(&maker.pubkey(), 10_000_000_000)
            .expect("airdrop");

        let taker = Keypair::new();
        svm.airdrop(&taker.pubkey(), 10_000_000_000)
            .expect("airdrop");

        let mint_a = CreateMint::new(&mut svm, &maker)
            .authority(&maker.pubkey())
            .decimals(6)
            .send()
            .unwrap();
        msg!("Mint A: {}\n", mint_a);

        let mint_b = CreateMint::new(&mut svm, &taker)
            .authority(&taker.pubkey())
            .decimals(6)
            .send()
            .unwrap();
        msg!("Mint B: {}\n", mint_b);

        let maker_ata_a = CreateAssociatedTokenAccount::new(&mut svm, &maker, &mint_a)
            .owner(&maker.pubkey())
            .send()
            .unwrap();
        msg!("Maker ATA A: {}\n", maker_ata_a);

        let maker_ata_b = get_associated_token_address(&maker.pubkey(), &mint_b);
        msg!("Maker ATA B: {}\n", maker_ata_b);

        let taker_ata_a = get_associated_token_address(&taker.pubkey(), &mint_a);
        msg!("Taker ATA A: {}\n", taker_ata_a);

        let taker_ata_b = CreateAssociatedTokenAccount::new(&mut svm, &taker, &mint_b)
            .owner(&taker.pubkey())
            .send()
            .unwrap();
        msg!("Taker ATA B: {}\n", taker_ata_b);

        MintTo::new(&mut svm, &maker, &mint_a, &maker_ata_a, SUPPLY)
            .send()
            .unwrap();
        MintTo::new(&mut svm, &taker, &mint_b, &taker_ata_b, SUPPLY)
            .send()
            .unwrap();

        let (escrow, _) = Pubkey::find_program_address(
            &[
                q3_26_escrow::constants::ESCROW_SEED,
                maker.pubkey().as_ref(),
                &SEED.to_le_bytes(),
            ],
            &program_id,
        );
        msg!("Escrow: {}\n", escrow);

        let vault = get_associated_token_address(&escrow, &mint_a);
        msg!("Vault: {}\n", vault);

        let mut setup = Self {
            svm,
            maker,
            taker,
            mint_a,
            mint_b,
            maker_ata_a,
            maker_ata_b,
            taker_ata_a,
            taker_ata_b,
            escrow,
            vault,
        };

        // Every test runs on a plausible clock rather than LiteSVM's default of 0.
        setup.warp_to(START_TIMESTAMP);

        setup
    }

    // --- instructions ---

    pub fn make(&mut self, expiration: i64) -> TxResult {
        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Make {
                maker: self.maker.pubkey(),
                mint_a: self.mint_a,
                mint_b: self.mint_b,
                maker_ata_a: self.maker_ata_a,
                escrow: self.escrow,
                vault: self.vault,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Make {
                id: SEED,
                deposit: DEPOSIT,
                token_b_wanted_amount: RECEIVE,
                expiration,
            }
            .data(),
        };
        let maker = self.maker.insecure_clone();
        self.send(ix, &maker)
    }

    pub fn refund(&mut self) -> TxResult {
        let maker = self.maker.insecure_clone();
        self.refund_as(&maker)
    }

    pub fn refund_as(&mut self, signer: &Keypair) -> TxResult {
        self.refund_with_mint_a(signer, self.mint_a)
    }

    pub fn refund_with_mint_a(&mut self, signer: &Keypair, mint_a: Pubkey) -> TxResult {
        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Refund {
                maker: signer.pubkey(),
                mint_a,
                maker_ata_a: get_associated_token_address(&signer.pubkey(), &mint_a),
                escrow: self.escrow,
                vault: self.vault,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Refund {}.data(),
        };
        self.send(ix, signer)
    }

    pub fn take(&mut self) -> TxResult {
        self.take_with(self.maker_ata_b, self.mint_a, self.mint_b)
    }

    pub fn take_with_maker_ata_b(&mut self, maker_ata_b: Pubkey) -> TxResult {
        self.take_with(maker_ata_b, self.mint_a, self.mint_b)
    }

    pub fn take_with_mints(&mut self, mint_a: Pubkey, mint_b: Pubkey) -> TxResult {
        let maker_ata_b = get_associated_token_address(&self.maker.pubkey(), &mint_b);
        self.take_with(maker_ata_b, mint_a, mint_b)
    }

    /// The token accounts are derived from the mints passed in, so substituting a mint
    /// yields a coherent account set and the failure lands on `has_one`, not on plumbing.
    pub fn take_with(&mut self, maker_ata_b: Pubkey, mint_a: Pubkey, mint_b: Pubkey) -> TxResult {
        let taker_pk = self.taker.pubkey();

        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Take {
                maker: self.maker.pubkey(),
                taker: taker_pk,
                mint_a,
                mint_b,
                taker_ata_a: get_associated_token_address(&taker_pk, &mint_a),
                taker_ata_b: get_associated_token_address(&taker_pk, &mint_b),
                maker_ata_b,
                escrow: self.escrow,
                vault: get_associated_token_address(&self.escrow, &mint_a),
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Take {}.data(),
        };
        let taker = self.taker.insecure_clone();
        self.send(ix, &taker)
    }

    pub fn update(&mut self, expiration: i64) -> TxResult {
        let maker = self.maker.insecure_clone();
        self.update_as(&maker, expiration)
    }

    pub fn update_as(&mut self, signer: &Keypair, expiration: i64) -> TxResult {
        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Update {
                maker: signer.pubkey(),
                escrow: self.escrow,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Update { expiration }.data(),
        };
        self.send(ix, signer)
    }

    fn send(&mut self, ix: Instruction, signer: &Keypair) -> TxResult {
        let message = Message::new(&[ix], Some(&signer.pubkey()));
        let recent_blockhash = self.svm.latest_blockhash();
        let transaction = Transaction::new(&[signer], message, recent_blockhash);
        self.svm.send_transaction(transaction)
    }

    // --- fixtures ---

    /// A mint the escrow knows nothing about, with every ATA it would need already created.
    /// Anchor evaluates `has_one` only after the account-init phase, so a substituted mint
    /// must come with a coherent set of accounts — otherwise the CPI that creates a missing
    /// ATA fails first, with an opaque `MissingAccount` instead of a constraint violation.
    pub fn new_foreign_mint(&mut self) -> Pubkey {
        let maker_pk = self.maker.pubkey();
        let taker_pk = self.taker.pubkey();
        let escrow = self.escrow;

        let mint = CreateMint::new(&mut self.svm, &self.maker)
            .authority(&maker_pk)
            .decimals(6)
            .send()
            .unwrap();

        for owner in [maker_pk, taker_pk, escrow] {
            CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &self.maker, &mint)
                .owner(&owner)
                .send()
                .unwrap();
        }

        mint
    }

    /// A funded third party with its own ATAs, ready to try to hijack an escrow.
    pub fn new_attacker(&mut self) -> Keypair {
        let attacker = Keypair::new();
        self.svm
            .airdrop(&attacker.pubkey(), 10_000_000_000)
            .expect("airdrop");

        let (mint_a, mint_b) = (self.mint_a, self.mint_b);
        let attacker_pk = attacker.pubkey();
        for mint in [mint_a, mint_b] {
            CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &attacker, &mint)
                .owner(&attacker_pk)
                .send()
                .unwrap();
        }

        attacker
    }

    /// Pre-creates `take`'s two destination ATAs, exercising the *skip* branch of
    /// `init_if_needed`. Without it, the program creates them itself — the *init* branch.
    pub fn create_destination_atas(&mut self) {
        let taker_pk = self.taker.pubkey();
        let maker_pk = self.maker.pubkey();
        let (mint_a, mint_b) = (self.mint_a, self.mint_b);

        CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &self.taker, &mint_a)
            .owner(&taker_pk)
            .send()
            .unwrap();
        CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &self.taker, &mint_b)
            .owner(&maker_pk)
            .send()
            .unwrap();
    }

    // --- state readers ---

    pub fn token_amount(&self, ata: &Pubkey) -> u64 {
        get_spl_account::<TokenAccount>(&self.svm, ata)
            .expect("token account")
            .amount
    }

    pub fn escrow_state(&self) -> q3_26_escrow::state::Escrow {
        let acc = self.svm.get_account(&self.escrow).unwrap();
        let mut data: &[u8] = &acc.data.as_ref();
        q3_26_escrow::state::Escrow::try_deserialize(&mut data).unwrap()
    }

    pub fn escrow_exists(&self) -> bool {
        self.svm
            .get_account(&self.escrow)
            .is_some_and(|account| account.lamports > 0)
    }

    pub fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    pub fn warp_to(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    pub fn assert_closed(&self, key: &Pubkey) {
        match self.svm.get_account(key) {
            None => {}
            Some(acc) => assert!(acc.lamports == 0 || acc.data.is_empty()),
        }
    }
}

// --- assertions ---

pub fn assert_escrow_error(result: TxResult, expected: EscrowError) {
    assert_custom_error(result, u32::from(expected));
}

/// Same, for errors Anchor raises from `#[account(...)]` attributes. They live below 6000,
/// in the ranges reserved by `anchor_lang::error::ErrorCode`.
pub fn assert_anchor_error(result: TxResult, expected: AnchorErrorCode) {
    assert_custom_error(result, u32::from(expected));
}

pub fn assert_custom_error(result: TxResult, expected_code: u32) {
    let failure = result.expect_err("transaction should have failed");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(0, InstructionError::Custom(expected_code)),
        "logs: {:#?}",
        failure.meta.logs,
    );
}

/// A settled swap: escrow and vault closed, and the four crossed balances.
pub fn assert_take_settled(setup: &Setup) {
    setup.assert_closed(&setup.escrow);
    setup.assert_closed(&setup.vault);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_a), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_b), SUPPLY - RECEIVE);
    assert_eq!(setup.token_amount(&setup.maker_ata_b), RECEIVE);
}

/// A rejected `make` must leave nothing behind.
pub fn assert_no_state_change(setup: &Setup) {
    assert!(!setup.escrow_exists(), "escrow account should not have been created");
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);
}
