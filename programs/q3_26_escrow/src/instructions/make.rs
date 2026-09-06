use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked
    }
};

use crate::{
    constants::{ESCROW_SEED, MAX_ESCROW_DURATION},
    error::EscrowError,
    state::Escrow,
};

#[derive(Accounts)]
#[instruction(id: u64)]
pub struct Make<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    #[account(
        init,
        payer = maker,
        space = Escrow::DISCRIMINATOR.len() + Escrow::INIT_SPACE,
        seeds = [ESCROW_SEED, maker.key().as_ref(), id.to_le_bytes().as_ref()],
        bump,
    )]
    pub escrow: Account<'info, Escrow>,

    #[account(
        mint::token_program = token_program
    )]
    pub mint_a: InterfaceAccount<'info, Mint>,

    #[account(
        mint::token_program = token_program
    )]
    pub mint_b: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program,
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>,

    #[account(
        init,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program,
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Make<'info> {
    pub fn populate_escrow(
        &mut self,
        id: u64,
        token_b_wanted_amount: u64,
        bumps: &MakeBumps,
        expiration: i64,
    ) -> Result<()> {
        require!(token_b_wanted_amount > 0, EscrowError::InvalidAmount);
        
        let current_time = Clock::get()?.unix_timestamp;
        require!(current_time < expiration, EscrowError::ExpirationInThePast);

        let max_expiration = current_time
            .checked_add(MAX_ESCROW_DURATION)
            .ok_or(EscrowError::ExpirationTooFar)?;
        require!(expiration <= max_expiration, EscrowError::ExpirationTooFar);

        self.escrow.set_inner(Escrow {
            id,
            maker: self.maker.key(),
            mint_a: self.mint_a.key(),
            mint_b: self.mint_b.key(),
            token_b_wanted_amount,
            bump: bumps.escrow,
            expiration,
        });

        Ok(())
    }

    pub fn deposit(&mut self, deposit: u64) -> Result<()> {
        require!(deposit > 0, EscrowError::InvalidAmount);

        let cpi_program = self.token_program.key();
        let transfer_accounts = TransferChecked {
            from: self.maker_ata_a.to_account_info(),
            mint: self.mint_a.to_account_info(),
            to: self.vault.to_account_info(),
            authority: self.maker.to_account_info(),
        };

        let cpi_ctx = CpiContext::new(
            cpi_program,
            transfer_accounts
        );

        transfer_checked(cpi_ctx, deposit, self.mint_a.decimals)
    }
}
