use anchor_lang::prelude::*;

use crate::{
    constants::ESCROW_SEED,
    error::EscrowError,
    state::Escrow,
};

#[derive(Accounts)]
pub struct Update<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    #[account(
        mut,
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
        require!(self.escrow.expiration < new_expiration, EscrowError::ExpirationShorterThanCurrent);

        self.escrow.expiration = new_expiration;

        Ok(())
    }
}
