use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Expiration must be in the future")]
    ExpirationInThePast,
    #[msg("New expiration must be later than the current one")]
    ExpirationNotExtended,
    #[msg("Expiration exceeds the maximum escrow duration")]
    ExpirationTooFar,
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("The offer has expired")]
    OfferExpired,
    #[msg("The offer hasn't expired yet")]
    OfferIsActive,
}
