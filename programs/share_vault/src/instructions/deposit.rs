use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        mint_to, transfer_checked, Mint, MintTo, TokenAccount, TokenInterface, TransferChecked,
    },
};

use crate::{
    error::VaultError,
    math::{mul_div, total_value},
    state::ShareVault,
    VAULT_SEED,
};

#[derive(Accounts)]
pub struct Deposit<'info> {
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
    #[account(
        init_if_needed,
        payer = user,
        associated_token::mint = share_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Deposit<'info> {
    pub fn deposit(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);

        // Shares are priced BEFORE the new money arrives:
        // first deposit 1:1, then shares = amount * supply / total_value.
        let supply = self.share_mint.supply;
        let value = total_value(self.asset_reserve.amount, self.position_reserve.amount)?;
        let shares = if supply == 0 || value == 0 {
            amount
        } else {
            mul_div(amount, supply, value)?
        };
        require!(shares > 0, VaultError::RoundsToZero);

        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.user_asset.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to: self.asset_reserve.to_account_info(),
                    authority: self.user.to_account_info(),
                },
            ),
            amount,
            self.asset_mint.decimals,
        )?;

        let manager = self.vault.manager;
        let seeds: &[&[u8]] = &[VAULT_SEED, manager.as_ref(), &[self.vault.bump]];
        mint_to(
            CpiContext::new_with_signer(
                self.token_program.key(),
                MintTo {
                    mint: self.share_mint.to_account_info(),
                    to: self.user_shares.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                &[seeds],
            ),
            shares,
        )
    }
}
