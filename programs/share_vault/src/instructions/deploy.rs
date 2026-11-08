use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked,
};

use crate::{error::VaultError, state::ShareVault, VAULT_SEED};

/// The only thing the manager can do: move idle asset into the market position.
/// It is an atomic 1:1 swap with the market (both legs or neither), so the
/// vault's value does not change and the manager never receives funds.
#[derive(Accounts)]
pub struct Deploy<'info> {
    pub manager: Signer<'info>,
    /// The market counterparty (in a real vault: a CPI into a lending or LP program).
    pub market: Signer<'info>,

    #[account(
        seeds = [VAULT_SEED, manager.key().as_ref()],
        bump = vault.bump,
        has_one = manager,
        has_one = asset_mint,
        has_one = position_mint
    )]
    pub vault: Box<Account<'info, ShareVault>>,

    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,
    pub position_mint: Box<InterfaceAccount<'info, Mint>>,

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
        token::mint = asset_mint,
        token::authority = market,
        token::token_program = token_program
    )]
    pub market_asset: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        mut,
        token::mint = position_mint,
        token::authority = market,
        token::token_program = token_program
    )]
    pub market_position: Box<InterfaceAccount<'info, TokenAccount>>,

    pub token_program: Interface<'info, TokenInterface>,
}

impl<'info> Deploy<'info> {
    pub fn deploy(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, VaultError::InvalidAmount);
        require!(
            amount <= self.asset_reserve.amount,
            VaultError::NotEnoughLiquidity
        );

        // Leg 1: the market delivers position tokens into the vault.
        transfer_checked(
            CpiContext::new(
                self.token_program.key(),
                TransferChecked {
                    from: self.market_position.to_account_info(),
                    mint: self.position_mint.to_account_info(),
                    to: self.position_reserve.to_account_info(),
                    authority: self.market.to_account_info(),
                },
            ),
            amount,
            self.position_mint.decimals,
        )?;

        // Leg 2: the vault pays the market in asset, signed by the vault PDA.
        let manager = self.manager.key();
        let seeds: &[&[u8]] = &[VAULT_SEED, manager.as_ref(), &[self.vault.bump]];
        transfer_checked(
            CpiContext::new_with_signer(
                self.token_program.key(),
                TransferChecked {
                    from: self.asset_reserve.to_account_info(),
                    mint: self.asset_mint.to_account_info(),
                    to: self.market_asset.to_account_info(),
                    authority: self.vault.to_account_info(),
                },
                &[seeds],
            ),
            amount,
            self.asset_mint.decimals,
        )
    }
}
