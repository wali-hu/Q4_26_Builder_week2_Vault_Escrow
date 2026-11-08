use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{error::VaultError, math::mul_div, state::ShareVault, VAULT_SEED};

/// The always-open exit: burn shares and receive the same fraction of EVERY
/// reserve (idle asset and position tokens). No price, no manager signature and
/// no idle liquidity needed, so it works even when everything is deployed.
#[derive(Accounts)]
pub struct RedeemInKind<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    #[account(
        seeds = [VAULT_SEED, vault.manager.as_ref()],
        bump = vault.bump,
        has_one = asset_mint,
        has_one = position_mint,
        has_one = share_mint
    )]
    pub vault: Box<Account<'info, ShareVault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,
    pub position_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mut)]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        mut,
        associated_token::mint = asset_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub asset_reserve: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = position_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub position_reserve: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = asset_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_asset: Box<InterfaceAccount<'info, TokenAccount>>,
    // The user may never have held the position token: create the ATA if needed.
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = position_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_position: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        associated_token::mint = share_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> RedeemInKind<'info> {
    pub fn redeem_in_kind(&mut self, shares: u64) -> Result<()> {
        require!(shares > 0, VaultError::InvalidAmount);

        // Same fraction of each reserve, rounded down (favours the vault).
        let supply = self.share_mint.supply;
        let asset_out = mul_div(shares, self.asset_reserve.amount, supply)?;
        let position_out = mul_div(shares, self.position_reserve.amount, supply)?;
        require!(asset_out > 0 || position_out > 0, VaultError::RoundsToZero);

        burn(
            CpiContext::new(
                self.token_program.key(),
                Burn {
                    mint: self.share_mint.to_account_info(),
                    from: self.user_shares.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            shares,
        )?;

        let manager = self.vault.manager;
        let seeds: &[&[u8]] = &[VAULT_SEED, manager.as_ref(), &[self.vault.bump]];

        if asset_out > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    self.token_program.key(),
                    TransferChecked {
                        from: self.asset_reserve.to_account_info(),
                        mint: self.asset_mint.to_account_info(),
                        to: self.user_asset.to_account_info(),
                        authority: self.vault.to_account_info(),
                    },
                    &[seeds],
                ),
                asset_out,
                self.asset_mint.decimals,
            )?;
        }
        if position_out > 0 {
            transfer_checked(
                CpiContext::new_with_signer(
                    self.token_program.key(),
                    TransferChecked {
                        from: self.position_reserve.to_account_info(),
                        mint: self.position_mint.to_account_info(),
                        to: self.user_position.to_account_info(),
                        authority: self.vault.to_account_info(),
                    },
                    &[seeds],
                ),
                position_out,
                self.position_mint.decimals,
            )?;
        }
        Ok(())
    }
}
