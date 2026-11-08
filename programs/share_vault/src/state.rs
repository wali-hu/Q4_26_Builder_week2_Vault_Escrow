use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct ShareVault {
    /// Can only move idle liquidity into the market position (deploy).
    /// Cannot withdraw, pause, or block redemptions.
    pub manager: Pubkey,
    /// Idle liquidity token (e.g. USDC).
    pub asset_mint: Pubkey,
    /// Receipt of the underlying market position (e.g. an LP or lending token).
    pub position_mint: Pubkey,
    /// Shares of the vault, minted by the vault PDA.
    pub share_mint: Pubkey,
    pub bump: u8,
    pub share_bump: u8,
}
