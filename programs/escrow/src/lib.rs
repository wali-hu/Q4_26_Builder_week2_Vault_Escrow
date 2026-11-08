pub mod constants;
pub mod error;
pub mod instructions;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("9HVLYbuu3hYnvDxtvcww62Ubsd7AJbMKab4wR3rRDq6J");

#[program]
pub mod escrow {
    use super::*;

    /// Maker opens an offer: deposits `deposit` of mint_a, asks for `receive`
    /// of mint_b, valid until `deadline`, optionally only for `taker`.
    pub fn make(
        ctx: Context<Make>,
        seed: u64,
        deposit: u64,
        receive: u64,
        deadline: i64,
        taker: Option<Pubkey>,
    ) -> Result<()> {
        ctx.accounts
            .init_escrow(seed, receive, deadline, taker, &ctx.bumps)?;
        ctx.accounts.deposit(deposit)
    }

    /// Taker accepts: pays `receive` of mint_b, gets the vault's mint_a.
    pub fn take(ctx: Context<Take>, expected_receive: u64) -> Result<()> {
        ctx.accounts.check_conditions(expected_receive)?;
        ctx.accounts.pay_maker()?;
        ctx.accounts.release_vault_and_close()
    }

    /// Maker cancels and gets the deposit back.
    pub fn refund(ctx: Context<Refund>) -> Result<()> {
        ctx.accounts.refund_and_close_vault()
    }

    /// Maker changes the price and/or deadline of an open offer.
    pub fn update(ctx: Context<Update>, new_receive: u64, new_deadline: i64) -> Result<()> {
        ctx.accounts.update(new_receive, new_deadline)
    }
}
