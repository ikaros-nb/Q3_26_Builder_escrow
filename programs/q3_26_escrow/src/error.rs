use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    #[msg("Expiration must be after current time")]
    ExpirationShorterThanCurrentTime,
    #[msg("Expiration must be extend the current one")]
    ExpirationShorterThanCurrent,
    #[msg("The offer has expired")]
    OfferExpired,
    #[msg("The offer hasn't expired yet")]
    OfferIsActive,
}
