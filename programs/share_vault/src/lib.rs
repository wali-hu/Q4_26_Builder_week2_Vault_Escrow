pub mod constants;
pub mod error;
pub mod instructions;
pub mod math;
pub mod state;

use anchor_lang::prelude::*;

pub use constants::*;
pub use instructions::*;
pub use state::*;

declare_id!("8JxKPoq7mLwbfjmqk12jPX2DvHSrcq5ncUuX7KENcRZY");

/// Non-custodial share vault.
///
/// The vault holds two things: idle `asset` and `position` tokens (the receipt of
/// an underlying market position). Users own the vault through `shares`.
/// - `withdraw` pays out in cash (asset) and needs idle liquidity.
/// - `redeem_in_kind` pays out a pro-rata slice of EVERY reserve. It needs no price,
///   no manager and no liquidity, so a user can always exit.
/// - The manager can only `deploy`: an atomic 1:1 swap of idle asset into position
///   tokens with a market. The manager can never take funds out.
#[program]
pub mod share_vault {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>) -> Result<()> {
        ctx.accounts.initialize(&ctx.bumps)
    }

    pub fn deposit(ctx: Context<Deposit>, amount: u64) -> Result<()> {
        ctx.accounts.deposit(amount)
    }

    pub fn deploy(ctx: Context<Deploy>, amount: u64) -> Result<()> {
        ctx.accounts.deploy(amount)
    }

    pub fn withdraw(ctx: Context<Withdraw>, shares: u64) -> Result<()> {
        ctx.accounts.withdraw(shares)
    }

    pub fn redeem_in_kind(ctx: Context<RedeemInKind>, shares: u64) -> Result<()> {
        ctx.accounts.redeem_in_kind(shares)
    }
}
