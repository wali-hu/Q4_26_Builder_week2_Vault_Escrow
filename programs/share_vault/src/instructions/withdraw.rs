use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    burn, transfer_checked, Burn, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{
    error::VaultError,
    math::{mul_div, total_value},
    state::ShareVault,
    VAULT_SEED,
};

/// Cash exit: burn shares, receive asset at the vault's value per share.
/// Works only while enough asset is idle in the vault.
#[derive(Accounts)]
pub struct Withdraw<'info> {
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
        mut,
        associated_token::mint = share_mint,
        associated_token::authority = user,
        associated_token::token_program = token_program
    )]
    pub user_shares: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> Withdraw<'info> {
    pub fn withdraw(&mut self, shares: u64) -> Result<()> {
        require!(shares > 0, VaultError::InvalidAmount);

        let value = total_value(self.asset_reserve.amount, self.position_reserve.amount)?;
        let amount = mul_div(shares, value, self.share_mint.supply)?;
        require!(amount > 0, VaultError::RoundsToZero);
        require!(
            amount <= self.asset_reserve.amount,
            VaultError::NotEnoughLiquidity
        );

        // The user's signature authorizes the burn; burning more than they hold
        // fails inside the Token program.
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
            amount,
            self.asset_mint.decimals,
        )
    }
}
