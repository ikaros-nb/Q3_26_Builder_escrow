pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("F7rM6r4imQypkGyrRnukZfB1iv8YHkPYKUzDSGE85U7A");

#[program]
pub mod q3_26_escrow {
    use super::*;

    pub fn make(ctx: Context<Make>) -> Result<()> {
        Ok(())
    }

    pub fn take(ctx: Context<Take>) -> Result<()> {
        Ok(())
    }

    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        Ok(())
    }
}
