use anchor_lang::{
    prelude::{msg, Pubkey}
};
use anchor_spl::{
    associated_token::get_associated_token_address,
};
use litesvm::LiteSVM;
use litesvm_token::{
    CreateMint, CreateAssociatedTokenAccount, MintTo,
};
use solana_keypair::Keypair;
use solana_signer::Signer;

const SEED: u64 = 42;
const SUPPLY: u64 = 1_000_000_000;

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
}
