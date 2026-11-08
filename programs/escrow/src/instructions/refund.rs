use anchor_lang::prelude::*;
use anchor_spl::token_interface::{
    close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
    TransferChecked,
};

use crate::{error::EscrowError, state::Escrow, ESCROW_SEED};

/// Refund rules:
/// - Before the deadline only the maker can cancel.
/// - After the deadline anyone can call it (a keeper, the taker, the maker), so an
///   expired offer is never stuck. The tokens and the rent always go to the maker;
///   the caller only pays the transaction fee.
#[derive(Accounts)]
pub struct Refund<'info> {
    pub caller: Signer<'info>,

    // Not a signer: anyone may refund after the deadline. Seeds + has_one make sure
    // this is the maker stored in the escrow, so funds cannot be sent elsewhere.
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    #[account(mint::token_program = token_program)]
    pub mint_a: InterfaceAccount<'info, Mint>,

    // Must be the maker's own ATA for mint_a: the tokens can only go back to the maker.
    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>,

    #[account(
        mut,
        close = maker,
        has_one = maker,
        has_one = mint_a,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Refund<'info> {
    pub fn check_caller(&self) -> Result<()> {
        if self.caller.key() != self.escrow.maker {
            require!(
                Clock::get()?.unix_timestamp >= self.escrow.deadline,
                EscrowError::OnlyMakerBeforeDeadline
            );
        }
        Ok(())
    }

    pub fn refund_and_close_vault(&mut self) -> Result<()> {
        let maker_key = self.maker.key();
        let seed_bytes = self.escrow.seed.to_le_bytes();
        let seeds: &[&[u8]] = &[
            ESCROW_SEED,
            maker_key.as_ref(),
            seed_bytes.as_ref(),
            &[self.escrow.bump],
        ];
        let signer_seeds = &[seeds];

        let cpi_ctx = CpiContext::new_with_signer(
            self.token_program.key(),
            TransferChecked {
                from: self.vault.to_account_info(),
                mint: self.mint_a.to_account_info(),
                to: self.maker_ata_a.to_account_info(),
                authority: self.escrow.to_account_info(),
            },
            signer_seeds,
        );
        transfer_checked(cpi_ctx, self.vault.amount, self.mint_a.decimals)?;

        let cpi_ctx = CpiContext::new_with_signer(
            self.token_program.key(),
            CloseAccount {
                account: self.vault.to_account_info(),
                destination: self.maker.to_account_info(),
                authority: self.escrow.to_account_info(),
            },
            signer_seeds,
        );
        close_account(cpi_ctx)
    }
}
