use crate::{
    constants::{STATE, VAULT_SEED},
    error::ErrorCode,
    state::VaultState,
};
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

#[derive(Accounts)]
pub struct Withdraw<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    // Seeds include the signer's key, so a user can only ever load their own
    // state. Passing someone else's state fails with ConstraintSeeds (2006).
    #[account(
        seeds = [STATE, user.key().as_ref()],
        bump = vault_state.state_bump
    )]
    pub vault_state: Account<'info, VaultState>,

    #[account(
        mut,
        seeds = [VAULT_SEED, user.key().as_ref()],
        bump = vault_state.vault_bump
    )]
    pub vault: SystemAccount<'info>,
    pub system_program: Program<'info, System>,
}

impl<'info> Withdraw<'info> {
    pub fn withdraw(&mut self, amount: u64) -> Result<()> {
        require!(amount > 0, ErrorCode::InvalidAmount);

        // The vault is a System-owned account with 0 bytes of data. After the
        // transfer it must hold either 0 lamports (the account disappears) or
        // at least the rent-exempt minimum; anything in between is rejected by
        // the runtime. Check it ourselves to return a readable error instead.
        let rent_min = Rent::get()?.minimum_balance(self.vault.data_len());
        let remaining = self
            .vault
            .lamports()
            .checked_sub(amount)
            .ok_or(ErrorCode::InsufficientFunds)?;
        require!(
            remaining == 0 || remaining >= rent_min,
            ErrorCode::WouldBreakRent
        );

        // The vault is a PDA, so it has no private key. The program signs for
        // it by passing the seeds + bump that derive its address.
        let user_key = self.user.key();
        let seeds: &[&[u8]] = &[
            VAULT_SEED,
            user_key.as_ref(),
            &[self.vault_state.vault_bump],
        ];
        let signer_seeds = &[seeds];

        let cpi_ctx = CpiContext::new_with_signer(
            self.system_program.key(),
            Transfer {
                from: self.vault.to_account_info(),
                to: self.user.to_account_info(),
            },
            signer_seeds,
        );

        transfer(cpi_ctx, amount)
    }
}
