use anchor_lang::{
    AccountDeserialize, InstructionData, ToAccountMetas, error::ErrorCode as AnchorErrorCode,
    prelude::{Pubkey, msg}, solana_program::{
        clock::Clock, instruction::{Instruction, error::InstructionError},
    }, system_program::ID as SYSTEM_PROGRAM_ID
};
use anchor_spl::{
    associated_token::{get_associated_token_address, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
};
use litesvm::{
    types::{FailedTransactionMetadata, TransactionMetadata},
    LiteSVM,
};
use litesvm_token::{
    get_spl_account, CreateAssociatedTokenAccount, CreateAssociatedTokenAccountIdempotent, CreateMint, MintTo, spl_token::{ID as TOKEN_PROGRAM_ID, state::Account as TokenAccount},
};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
use solana_transaction_error::TransactionError;
use q3_26_escrow::{error::EscrowError, MAX_ESCROW_DURATION};

const SEED: u64 = 42;
const SUPPLY: u64 = 1_000_000_000;
const DEPOSIT: u64 = 1_000_000;
const RECEIVE: u64 = 1_000_000;
const TEN_DAYS: i64 = 864_000; // 10 days in seconds, well under MAX_ESCROW_DURATION (30 days)
// A plausible wall-clock timestamp. LiteSVM starts the Clock sysvar at 0, which would make
// "now - 10 days" a negative unix timestamp — an input no real cluster can produce.
const START_TIMESTAMP: i64 = 1_788_683_523;

struct Setup {
    svm: LiteSVM,
    maker: Keypair,
    taker: Keypair,
    mint_a: Pubkey,
    mint_b: Pubkey,
    maker_ata_a: Pubkey,
    maker_ata_b: Pubkey,
    taker_ata_a: Pubkey,
    taker_ata_b: Pubkey,
    escrow: Pubkey,
    vault: Pubkey,
}

impl Setup {
    fn new() -> Self {
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

    fn make(&mut self, expiration: i64) -> Result<TransactionMetadata, FailedTransactionMetadata> {
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
        let Self { svm, maker, .. } = self;
        let message = Message::new(&[ix], Some(&maker.pubkey()));
        let recent_blockhash = svm.latest_blockhash();
        let transaction = Transaction::new(
            &[&maker],
            message,
            recent_blockhash,
        );
        svm.send_transaction(transaction)
    }

    fn refund(&mut self) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        let maker = self.maker.insecure_clone();
        self.refund_as(&maker)
    }

    fn refund_as(&mut self, signer: &Keypair) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        self.refund_with_mint_a(signer, self.mint_a)
    }

    fn refund_with_mint_a(&mut self, signer: &Keypair, mint_a: Pubkey) -> Result<TransactionMetadata, FailedTransactionMetadata> {
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
        let message = Message::new(&[ix], Some(&signer.pubkey()));
        let recent_blockhash = self.svm.latest_blockhash();
        let transaction = Transaction::new(&[signer], message, recent_blockhash);
        self.svm.send_transaction(transaction)
    }

    /// A mint the escrow knows nothing about, with every ATA it would need already created.
    /// Anchor evaluates `has_one` only after the account-init phase, so a substituted mint
    /// must come with a coherent set of accounts — otherwise the CPI that creates a missing
    /// ATA fails first, with an opaque `MissingAccount` instead of a constraint violation.
    fn new_foreign_mint(&mut self) -> Pubkey {
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

    fn create_destination_atas(&mut self) {
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

    fn take(&mut self) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        self.take_with(self.maker_ata_b, self.mint_a, self.mint_b)
    }

    fn take_with_maker_ata_b(&mut self, maker_ata_b: Pubkey) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        self.take_with(maker_ata_b, self.mint_a, self.mint_b)
    }

    fn take_with_mints(&mut self, mint_a: Pubkey, mint_b: Pubkey) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        let maker_ata_b = get_associated_token_address(&self.maker.pubkey(), &mint_b);
        self.take_with(maker_ata_b, mint_a, mint_b)
    }

    /// The token accounts are derived from the mints passed in, so substituting a mint
    /// yields a coherent account set and the failure lands on `has_one`, not on plumbing.
    fn take_with(&mut self, maker_ata_b: Pubkey, mint_a: Pubkey, mint_b: Pubkey) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        let taker_pk = self.taker.pubkey();
        let maker_pk = self.maker.pubkey();
        let taker_ata_a = get_associated_token_address(&taker_pk, &mint_a);
        let taker_ata_b = get_associated_token_address(&taker_pk, &mint_b);
        let vault = get_associated_token_address(&self.escrow, &mint_a);

        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Take {
                maker: maker_pk,
                taker: taker_pk,
                mint_a,
                mint_b,
                taker_ata_a,
                taker_ata_b,
                maker_ata_b,
                escrow: self.escrow,
                vault,
                associated_token_program: ASSOCIATED_TOKEN_PROGRAM_ID,
                token_program: TOKEN_PROGRAM_ID,
                system_program: SYSTEM_PROGRAM_ID,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Take {}.data(),
        };
        let Self { svm, taker, .. } = self;
        let message = Message::new(&[ix], Some(&taker.pubkey()));
        let recent_blockhash = svm.latest_blockhash();
        let transaction = Transaction::new(
            &[&taker],
            message,
            recent_blockhash,
        );
        svm.send_transaction(transaction)
    }

    fn update(&mut self, expiration: i64) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        let maker = self.maker.insecure_clone();
        self.update_as(&maker, expiration)
    }

    fn update_as(&mut self, signer: &Keypair, expiration: i64) -> Result<TransactionMetadata, FailedTransactionMetadata> {
        let ix = Instruction {
            program_id: q3_26_escrow::id(),
            accounts: q3_26_escrow::accounts::Update {
                maker: signer.pubkey(),
                escrow: self.escrow,
            }
            .to_account_metas(None),
            data: q3_26_escrow::instruction::Update {
                expiration
            }.data(),
        };
        let message = Message::new(&[ix], Some(&signer.pubkey()));
        let recent_blockhash = self.svm.latest_blockhash();
        let transaction = Transaction::new(&[signer], message, recent_blockhash);
        self.svm.send_transaction(transaction)
    }

    fn new_attacker(&mut self) -> Keypair {
        let attacker = Keypair::new();
        self.svm.airdrop(&attacker.pubkey(), 10_000_000_000).expect("airdrop");

        let (mint_a, mint_b) = (self.mint_a, self.mint_b);
        let attacker_pk = attacker.pubkey();
        CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &attacker, &mint_a)
            .owner(&attacker_pk)
            .send()
            .unwrap();
        CreateAssociatedTokenAccountIdempotent::new(&mut self.svm, &attacker, &mint_b)
            .owner(&attacker_pk)
            .send()
            .unwrap();

        attacker
    }

    fn token_amount(&self, ata: &Pubkey) -> u64 {
        get_spl_account::<TokenAccount>(&self.svm, ata)
            .expect("token account")
            .amount
    }

    fn escrow_state(&self) -> q3_26_escrow::state::Escrow {
        let acc = self.svm.get_account(&self.escrow).unwrap();
        let mut data: &[u8] = &acc.data.as_ref();
        q3_26_escrow::state::Escrow::try_deserialize(&mut data).unwrap()
    }

    fn escrow_exists(&self) -> bool {
        self.svm
            .get_account(&self.escrow)
            .is_some_and(|account| account.lamports > 0)
    }

    fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }

    fn warp_to(&mut self, unix_timestamp: i64) {
        let mut clock = self.svm.get_sysvar::<Clock>();
        clock.unix_timestamp = unix_timestamp;
        self.svm.set_sysvar::<Clock>(&clock);
    }

    fn assert_closed(&self, key: &Pubkey) {
        match self.svm.get_account(key) {
            None => {}
            Some(acc) => assert!(acc.lamports == 0 || acc.data.is_empty()),
        }
    }
}

// --- make ---

#[test]
fn test_make() {
    let mut setup = Setup::new();
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);

    let expiration = setup.now().saturating_add(TEN_DAYS);
    let transaction = setup.make(expiration)
        .expect("make should succeed");

    msg!("\n\nMake transaction sucessfull");
    msg!("CUs Consumed: {}", transaction.compute_units_consumed);
    msg!("Tx Signature: {}", transaction.signature);

    let escrow = setup.escrow_state();
    assert_eq!((escrow.id, escrow.token_b_wanted_amount), (SEED, RECEIVE));
    assert_eq!(escrow.expiration, expiration);
    assert_eq!(escrow.maker, setup.maker.pubkey());
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
}

#[test]
fn test_make_succeeds_at_max_expiration() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(MAX_ESCROW_DURATION);
    setup.make(expiration)
        .expect("make should succeed");

    assert_eq!(setup.escrow_state().expiration, expiration);
}

#[test]
fn test_make_fails_when_expiration_in_the_past() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_sub(TEN_DAYS);
    assert_escrow_error(setup.make(expiration), EscrowError::ExpirationInThePast);

    assert_no_state_change(&setup);
}

#[test]
fn test_make_fails_when_expiration_equals_now() {
    let mut setup = Setup::new();

    let expiration = setup.now();
    assert_escrow_error(setup.make(expiration), EscrowError::ExpirationInThePast);

    assert_no_state_change(&setup);
}

#[test]
fn test_make_fails_when_expiration_exceeds_max() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(MAX_ESCROW_DURATION).saturating_add(1);
    assert_escrow_error(setup.make(expiration), EscrowError::ExpirationTooFar);

    assert_no_state_change(&setup);
}

// --- refund ---

#[test]
fn test_refund() {
    let mut setup = Setup::new();
    
    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");
    setup.warp_to(expiration);
    setup.refund()
        .expect("refund should succeed");

    setup.assert_closed(&setup.escrow);
    setup.assert_closed(&setup.vault);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);
}

#[test]
fn test_refund_fails_when_offer_is_active() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.warp_to(expiration.saturating_sub(1));
    assert_escrow_error(setup.refund(), EscrowError::OfferIsActive);

    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
}

// --- take ---

#[test]
fn test_take() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.take()
        .expect("take should succeed");

    assert_take_settled(&setup);
}

#[test]
fn test_take_succeeds_when_destination_atas_exist() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.create_destination_atas();
    setup.take()
        .expect("take should succeed");

    assert_take_settled(&setup);
}

#[test]
fn test_take_succeeds_just_before_expiration() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.warp_to(expiration.saturating_sub(1));
    setup.take()
        .expect("take should succeed one second before expiration");

    assert_take_settled(&setup);
}

#[test]
fn test_take_fails_when_offer_has_expired() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.create_destination_atas();
    setup.warp_to(expiration);
    assert_escrow_error(setup.take(), EscrowError::OfferExpired);

    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_a), 0);
    assert_eq!(setup.token_amount(&setup.taker_ata_b), SUPPLY);
    assert_eq!(setup.token_amount(&setup.maker_ata_b), 0);
}

// --- update ---

#[test]
fn test_update_extends_expiration() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let extended = expiration.saturating_add(TEN_DAYS);
    setup.update(extended)
        .expect("update should succeed");

    assert_eq!(setup.escrow_state().expiration, extended);
}

// The 30-day bound is measured from `now`, not from creation.
#[test]
fn test_update_succeeds_at_max_expiration() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let max_expiration = setup.now().saturating_add(MAX_ESCROW_DURATION);
    setup.update(max_expiration)
        .expect("update should succeed at the inclusive bound");

    assert_eq!(setup.escrow_state().expiration, max_expiration);
}

#[test]
fn test_update_fails_when_expiration_exceeds_max() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let too_far = setup.now().saturating_add(MAX_ESCROW_DURATION).saturating_add(1);
    assert_escrow_error(setup.update(too_far), EscrowError::ExpirationTooFar);

    assert_eq!(setup.escrow_state().expiration, expiration);
}

// `update` only extends: shortening the deadline would let the maker pull an offer out
// from under a taker mid-transaction.
#[test]
fn test_update_fails_when_expiration_is_not_extended() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    // Equal to the current deadline: refused, the constraint is strict.
    assert_escrow_error(setup.update(expiration), EscrowError::ExpirationNotExtended);
    // Earlier but still in the future: refused too.
    assert_escrow_error(setup.update(expiration.saturating_sub(1)), EscrowError::ExpirationNotExtended);

    assert_eq!(setup.escrow_state().expiration, expiration);
}

#[test]
fn test_update_fails_when_expiration_in_the_past() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    assert_escrow_error(setup.update(setup.now().saturating_sub(1)), EscrowError::ExpirationInThePast);

    assert_eq!(setup.escrow_state().expiration, expiration);
}

// Expiry is terminal: the maker cannot resurrect a dead offer by extending it.
#[test]
fn test_update_fails_when_offer_has_expired() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    setup.warp_to(expiration);
    assert_escrow_error(
        setup.update(expiration.saturating_add(TEN_DAYS)),
        EscrowError::OfferExpired,
    );

    assert_eq!(setup.escrow_state().expiration, expiration);
}

// --- Authority: who may call what ---

// Guarded by the PDA seeds, not by a `require!`: the escrow is derived from the maker's key,
// so an attacker's escrow is never the maker's.
#[test]
fn test_refund_fails_for_non_maker() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");
    setup.warp_to(expiration);

    let attacker = setup.new_attacker();
    assert_anchor_error(setup.refund_as(&attacker), AnchorErrorCode::ConstraintSeeds);

    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    setup.refund().expect("the real maker can still refund");
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);
}

// Extending the window during which the deposit stays locked is the maker's call alone.
#[test]
fn test_update_fails_for_non_maker() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let attacker = setup.new_attacker();
    assert_anchor_error(
        setup.update_as(&attacker, expiration.saturating_add(TEN_DAYS)),
        AnchorErrorCode::ConstraintSeeds,
    );

    assert_eq!(setup.escrow_state().expiration, expiration);
}

// The taker signs legitimately but redirects the token B payment to itself, walking away
// with token A for free. Blocked by `associated_token::authority = maker`, not a `require!`.
#[test]
fn test_take_fails_when_payment_is_redirected() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let attacker = setup.new_attacker();
    let attacker_ata_b = get_associated_token_address(&attacker.pubkey(), &setup.mint_b);
    assert_anchor_error(
        setup.take_with_maker_ata_b(attacker_ata_b),
        AnchorErrorCode::ConstraintTokenOwner,
    );

    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    assert_eq!(setup.token_amount(&attacker_ata_b), 0);
}

// The escrow records which mints it trades. Substituting one lets a caller settle against
// a token the other side never agreed to, so `has_one = mint_a` / `has_one = mint_b` must
// reject any mint account that disagrees with the stored state.
#[test]
fn test_refund_fails_with_foreign_mint_a() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");
    setup.warp_to(expiration);

    let foreign_mint = setup.new_foreign_mint();
    let maker = setup.maker.insecure_clone();
    assert_anchor_error(
        setup.refund_with_mint_a(&maker, foreign_mint),
        AnchorErrorCode::ConstraintHasOne,
    );

    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
}

#[test]
fn test_take_fails_with_foreign_mint_a() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let foreign_mint = setup.new_foreign_mint();
    let mint_b = setup.mint_b;
    assert_anchor_error(
        setup.take_with_mints(foreign_mint, mint_b),
        AnchorErrorCode::ConstraintHasOne,
    );

    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
}

#[test]
fn test_take_fails_with_foreign_mint_b() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let foreign_mint = setup.new_foreign_mint();
    let mint_a = setup.mint_a;
    assert_anchor_error(
        setup.take_with_mints(mint_a, foreign_mint),
        AnchorErrorCode::ConstraintHasOne,
    );

    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_b), SUPPLY);
}

fn assert_escrow_error(
    result: Result<TransactionMetadata, FailedTransactionMetadata>,
    expected: EscrowError,
) {
    assert_custom_error(result, u32::from(expected));
}

fn assert_anchor_error(
    result: Result<TransactionMetadata, FailedTransactionMetadata>,
    expected: AnchorErrorCode,
) {
    assert_custom_error(result, u32::from(expected));
}

fn assert_custom_error(
    result: Result<TransactionMetadata, FailedTransactionMetadata>,
    expected_code: u32,
) {
    let failure = result.expect_err("transaction should have failed");
    assert_eq!(
        failure.err,
        TransactionError::InstructionError(0, InstructionError::Custom(expected_code)),
        "logs: {:#?}",
        failure.meta.logs,
    );
}

fn assert_take_settled(setup: &Setup) {
    setup.assert_closed(&setup.escrow);
    setup.assert_closed(&setup.vault);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_a), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_b), SUPPLY - RECEIVE);
    assert_eq!(setup.token_amount(&setup.maker_ata_b), RECEIVE);
}

fn assert_no_state_change(setup: &Setup) {
    assert!(!setup.escrow_exists(), "escrow account should not have been created");
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);
}