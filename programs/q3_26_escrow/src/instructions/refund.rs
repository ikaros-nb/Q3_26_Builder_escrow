use anchor_lang::prelude::*;

use crate::{
    constants::ESCROW_SEED,
    state::Escrow
};

#[derive(Accounts)]
pub struct Refund<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,

    pub system_program: Program<'info, System>,
}

impl<'info> Refund<'info> {}