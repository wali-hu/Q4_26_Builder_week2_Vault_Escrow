use crate::{
    constants::{STATE, VAULT_SEED},
    state::VaultState,
};
use anchor_lang::{
    prelude::*,
    system_program::{transfer, Transfer},
};

#[derive(Accounts)]
pub struct Close<'info> {
    #[account(mut)]
    pub user: Signer<'info>,

    // close = user: after the handler, Anchor moves all of vault_state's
    // lamports (its rent) to user, sets its data to empty and hands it back
    // to the System Program. Works because this program owns vault_state.
    #[account(
        mut,
        seeds = [STATE, user.key().as_ref()],
        bump = vault_state.state_bump,
        close = user
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

impl<'info> Close<'info> {
    pub fn close(&mut self) -> Result<()> {
        // The vault is owned by the System Program, not by us, so `close = `
        // cannot be used on it. Empty it with a signed System transfer of every
        // lamport; an account with 0 lamports is removed by the runtime.
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

        transfer(cpi_ctx, self.vault.lamports())
    }
}
