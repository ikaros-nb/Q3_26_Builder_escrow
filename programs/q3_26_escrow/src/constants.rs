use anchor_lang::prelude::*;

#[constant]
pub const ESCROW_SEED: &[u8] = b"escrow";

#[constant]
pub const MAX_ESCROW_DURATION: i64 = 2_592_000; // 30 days, in seconds