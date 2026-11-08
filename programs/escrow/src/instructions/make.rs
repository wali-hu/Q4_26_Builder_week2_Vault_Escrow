use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{transfer_checked, Mint, TokenAccount, TokenInterface, TransferChecked},
};

use crate::{error::EscrowError, state::Escrow, ESCROW_SEED};

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct Make<'info> {
    #[account(mut)]
    pub maker: Signer<'info>,

    // Both mints must belong to the token program passed in (Token or Token-2022).
    #[account(mint::token_program = token_program)]
    pub mint_a: InterfaceAccount<'info, Mint>,
    #[account(mint::token_program = token_program)]
    pub mint_b: InterfaceAccount<'info, Mint>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_a: InterfaceAccount<'info, TokenAccount>,

    // The offer. seed lets the same maker have many open offers.
    #[account(
        init,
        payer = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), seed.to_le_bytes().as_ref()],
        bump,
        space = Escrow::DISCRIMINATOR.len() + Escrow::INIT_SPACE
    )]
    pub escrow: Account<'info, Escrow>,

    // Holds the maker's tokens. Its authority is the escrow PDA, so only this
    // program (signing with the escrow seeds) can move them out.
    #[account(
        init,
        payer = maker,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: InterfaceAccount<'info, TokenAccount>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Make<'info> {
    pub fn init_escrow(
        &mut self,
        seed: u64,
        receive: u64,
        deadline: i64,
        taker: Option<Pubkey>,
        bumps: &MakeBumps,
    ) -> Result<()> {
        require!(receive > 0, EscrowError::InvalidAmount);
        require!(
            deadline > Clock::get()?.unix_timestamp,
            EscrowError::InvalidDeadline
        );

        self.escrow.set_inner(Escrow {
            seed,
            maker: self.maker.key(),
            mint_a: self.mint_a.key(),
            mint_b: self.mint_b.key(),
            receive,
            deadline,
            taker,
            bump: bumps.escrow,
        });
        Ok(())
    }

    pub fn deposit(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, EscrowError::InvalidAmount);

        // The maker signed the transaction, so a plain CpiContext is enough.
        let cpi_ctx = CpiContext::new(
            self.token_program.key(),
            TransferChecked {
                from: self.maker_ata_a.to_account_info(),
                mint: self.mint_a.to_account_info(),
                to: self.vault.to_account_info(),
                authority: self.maker.to_account_info(),
            },
        );
        transfer_checked(cpi_ctx, amount, self.mint_a.decimals)
    }
}
