mod common;

use common::*;

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
