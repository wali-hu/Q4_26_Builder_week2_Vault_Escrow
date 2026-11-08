use anchor_lang::prelude::*;
use anchor_spl::{
    associated_token::AssociatedToken,
    token_interface::{
        close_account, transfer_checked, CloseAccount, Mint, TokenAccount, TokenInterface,
        TransferChecked,
    },
};

use crate::{error::EscrowError, state::Escrow, ESCROW_SEED};

#[derive(Accounts)]
pub struct Take<'info> {
    #[account(mut)]
    pub taker: Signer<'info>,

    // Receives mint_b and the rent of the closed escrow and vault.
    #[account(mut)]
    pub maker: SystemAccount<'info>,

    // Boxed: these structs are large and the stack frame is limited to 4 KB.
    #[account(mint::token_program = token_program)]
    pub mint_a: Box<InterfaceAccount<'info, Mint>>,
    #[account(mint::token_program = token_program)]
    pub mint_b: Box<InterfaceAccount<'info, Mint>>,

    // Taker may not have an ATA for mint_a yet: create it, taker pays.
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = mint_a,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_a: Box<InterfaceAccount<'info, TokenAccount>>,

    #[account(
        mut,
        associated_token::mint = mint_b,
        associated_token::authority = taker,
        associated_token::token_program = token_program
    )]
    pub taker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // Maker may not have an ATA for mint_b yet: taker pays to create it.
    #[account(
        init_if_needed,
        payer = taker,
        associated_token::mint = mint_b,
        associated_token::authority = maker,
        associated_token::token_program = token_program
    )]
    pub maker_ata_b: Box<InterfaceAccount<'info, TokenAccount>>,

    // has_one checks that the accounts passed in are the ones stored in the
    // offer, so a taker cannot swap in a cheaper mint or a different maker.
    #[account(
        mut,
        close = maker,
        has_one = maker,
        has_one = mint_a,
        has_one = mint_b,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Box<Account<'info, Escrow>>,

    #[account(
        mut,
        associated_token::mint = mint_a,
        associated_token::authority = escrow,
        associated_token::token_program = token_program
    )]
    pub vault: Box<InterfaceAccount<'info, TokenAccount>>,

    pub associated_token_program: Program<'info, AssociatedToken>,
    pub token_program: Interface<'info, TokenInterface>,
    pub system_program: Program<'info, System>,
}

impl<'info> Take<'info> {
    /// All the conditions of the timed escrow, checked before any token moves.
    pub fn check_conditions(&self, expected_receive: u64) -> Result<()> {
        let escrow = &self.escrow;

        require_keys_neq!(self.taker.key(), escrow.maker, EscrowError::MakerCannotTake);

        // Clock sysvar: the deadline is a unix timestamp in seconds.
        require!(
            Clock::get()?.unix_timestamp < escrow.deadline,
            EscrowError::EscrowExpired
        );

        // Signature condition: if the maker named a taker, only that wallet can sign.
        if let Some(allowed) = escrow.taker {
            require_keys_eq!(self.taker.key(), allowed, EscrowError::NotDesignatedTaker);
        }

        // update() can change the price. The taker passes the price they saw,
        // so a maker cannot raise it while the taker's transaction is in flight.
        require_eq!(escrow.receive, expected_receive, EscrowError::TermsChanged);
        Ok(())
    }

    /// Taker pays the maker in mint_b.
    pub fn pay_maker(&mut self) -> Result<()> {
        let cpi_ctx = CpiContext::new(
            self.token_program.key(),
            TransferChecked {
                from: self.taker_ata_b.to_account_info(),
                mint: self.mint_b.to_account_info(),
                to: self.maker_ata_b.to_account_info(),
                authority: self.taker.to_account_info(),
            },
        );
        transfer_checked(cpi_ctx, self.escrow.receive, self.mint_b.decimals)
    }

    /// Vault pays the taker in mint_a, then the vault is closed (rent to maker).
    pub fn release_vault_and_close(&mut self) -> Result<()> {
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
                to: self.taker_ata_a.to_account_info(),
                authority: self.escrow.to_account_info(),
            },
            signer_seeds,
        );
        transfer_checked(cpi_ctx, self.vault.amount, self.mint_a.decimals)?;

        // A token account can only be closed by its token program, and only
        // when empty. The escrow PDA is its authority, so it signs again.
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
