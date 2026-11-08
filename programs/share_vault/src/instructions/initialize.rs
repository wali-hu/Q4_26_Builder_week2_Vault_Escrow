use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{Mint, TokenAccount, TokenInterface},
};

use crate::{state::ShareVault, SHARES_SEED, VAULT_SEED};

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(mut)]
    pub manager: Signer<'info>,

    #[account(mint::token_program = token_program)]
    pub asset_mint: Box<InterfaceAccount<'info, Mint>>,
    #[account(mint::token_program = token_program)]
    pub position_mint: Box<InterfaceAccount<'info, Mint>>,

    #[account(
        init,
        payer = manager,
        seeds = [VAULT_SEED, manager.key().as_ref()],
        bump,
        space = ShareVault::DISCRIMINATOR.len() + ShareVault::INIT_SPACE
    )]
    pub vault: Box<Account<'info, ShareVault>>,

    // Shares: a mint whose only authority is the vault PDA, so shares exist only
    // when the program mints them against a deposit.
    #[account(
        init,
        payer = manager,
        seeds = [SHARES_SEED, vault.key().as_ref()],
        bump,
        mint::decimals = asset_mint.decimals,
        mint::authority = vault,
        mint::token_program = token_program
    )]
    pub share_mint: Box<InterfaceAccount<'info, Mint>>,

    // The two reserves, both owned (as token authority) by the vault PDA.
    #[account(
        init,
        payer = manager,
        associated_token::mint = asset_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub asset_reserve: Box<InterfaceAccount<'info, TokenAccount>>,
    #[account(
        init,
        payer = manager,
        associated_token::mint = position_mint,
        associated_token::authority = vault,
        associated_token::token_program = token_program
    )]
    pub position_reserve: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Initialize<'info> {
    pub fn initialize(&mut self, bumps: &InitializeBumps) -> Result<()> {
        self.vault.set_inner(ShareVault {
            manager: self.manager.key(),
            asset_mint: self.asset_mint.key(),
            position_mint: self.position_mint.key(),
            share_mint: self.share_mint.key(),
            bump: bumps.vault,
            share_bump: bumps.share_mint,
        });
        Ok(())
    }
}
