mod common;

use common::*;

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

// The escrow records which mints it trades. Substituting one lets a caller settle against
// a token the other side never agreed to, so `has_one = mint_a` must reject any mint
// account that disagrees with the stored state.
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
