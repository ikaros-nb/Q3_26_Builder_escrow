use anchor_lang::{
    AccountDeserialize, InstructionData, ToAccountMetas, prelude::{Pubkey, msg}, solana_program::{
        clock::Clock, instruction::Instruction,
    }, system_program::ID as SYSTEM_PROGRAM_ID
};
use anchor_spl::{
    associated_token::{get_associated_token_address, ID as ASSOCIATED_TOKEN_PROGRAM_ID},
};
use litesvm::LiteSVM;
use litesvm_token::{
    get_spl_account, CreateAssociatedTokenAccount, CreateMint, MintTo, spl_token::{ID as TOKEN_PROGRAM_ID, state::Account as TokenAccount},
};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

const SEED: u64 = 42;
const SUPPLY: u64 = 1_000_000_000;
const DEPOSIT: u64 = 1_000_000;
const RECEIVE: u64 = 1_000_000;
const TEN_DAYS: i64 = 864_000; // 10 days in seconds, well under MAX_ESCROW_DURATION (30 days)

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

        Self {
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
        }
    }

    fn make(&mut self, expiration: i64) {
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
        let tx = svm.send_transaction(transaction).unwrap();

        msg!("\n\nMake transaction sucessfull");
        msg!("CUs Consumed: {}", tx.compute_units_consumed);
        msg!("Tx Signature: {}", tx.signature);
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

    fn now(&self) -> i64 {
        self.svm.get_sysvar::<Clock>().unix_timestamp
    }
}

#[test]
fn test_make_succeeds_with_future_expiration() {
    let mut setup = Setup::new();
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY);

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration);

    let escrow = setup.escrow_state();
    assert_eq!((escrow.id, escrow.token_b_wanted_amount), (SEED, RECEIVE));
    assert_eq!(escrow.expiration, expiration);
    assert_eq!(escrow.maker, setup.maker.pubkey());
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
}
