use anchor_lang::prelude::*;

use crate::{
    constants::{ESCROW_SEED, MAX_ESCROW_DURATION},
    error::EscrowError,
    state::Escrow,
};

#[derive(Accounts)]
pub struct Update<'info> {
    pub maker: Signer<'info>,

    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.id.to_le_bytes().as_ref()],
        bump = escrow.bump,
    )]
    pub escrow: Account<'info, Escrow>,
}

impl<'info> Update<'info> {
    pub fn update_escrow(
        &mut self,
        new_expiration: i64,
    ) -> Result<()> {
        let current_time = Clock::get()?.unix_timestamp;
        require!(current_time < self.escrow.expiration, EscrowError::OfferExpired);
        require!(current_time < new_expiration, EscrowError::ExpirationInThePast);
        require!(self.escrow.expiration < new_expiration, EscrowError::ExpirationNotExtended);

        let max_expiration = current_time
            .checked_add(MAX_ESCROW_DURATION)
            .ok_or(EscrowError::ExpirationTooFar)?;
        require!(new_expiration <= max_expiration, EscrowError::ExpirationTooFar);

        self.escrow.expiration = new_expiration;

        Ok(())
    }
}
