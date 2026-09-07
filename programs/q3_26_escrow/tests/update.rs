mod common;

use common::*;

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
    // Extending must not touch the deposit.
    assert_eq!(setup.token_amount(&setup.vault), DEPOSIT);
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
