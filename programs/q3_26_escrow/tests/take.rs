mod common;

use common::*;

// `taker_ata_a` and `maker_ata_b` do not exist yet, so the program must create them:
// exercises the *init* branch of `init_if_needed`.
#[test]
fn test_take() {
    let mut setup = Setup::new();

    let expiration = setup.now().saturating_add(TEN_DAYS);
    setup.make(expiration)
        .expect("make should succeed");

    let transaction = setup.take()
        .expect("take should succeed");

    msg!("\n\nTake transaction sucessfull");
    msg!("CUs Consumed: {}", transaction.compute_units_consumed);
    msg!("Tx Signature: {}", transaction.signature);

    assert_take_settled(&setup);
}

// Same swap, but both destination ATAs already exist: exercises the *skip* branch. The
// outcome must be identical — that is what shows `init_if_needed` reinitializes nothing.
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

// `require!(current_time < escrow.expiration)` is strict, so the last second before the
// deadline must still go through. With the test below, the bound is pinned on both sides.
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

    // Destination ATAs created up front so the balances below have accounts to read.
    setup.create_destination_atas();
    setup.warp_to(expiration);
    assert_escrow_error(setup.take(), EscrowError::OfferExpired);

    // Neither side of the swap moved: atomicity, no half-trade.
    assert!(setup.escrow_exists(), "escrow should still be open");
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
    assert_eq!(setup.token_amount(&setup.maker_ata_a), SUPPLY - DEPOSIT);
    assert_eq!(setup.token_amount(&setup.taker_ata_a), 0);
    assert_eq!(setup.token_amount(&setup.taker_ata_b), SUPPLY);
    assert_eq!(setup.token_amount(&setup.maker_ata_b), 0);
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

// A coherent account set built around a mint the escrow never recorded. Only
// `has_one = mint_a` / `has_one = mint_b` stands between that and a settled swap.
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
