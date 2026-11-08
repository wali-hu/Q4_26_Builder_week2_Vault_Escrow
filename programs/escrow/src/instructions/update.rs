use anchor_lang::prelude::*;

use crate::{error::EscrowError, state::Escrow, ESCROW_SEED};

/// What is updatable, and why:
/// - `receive` (the price): pure state, no tokens move.
/// - `deadline`: pure state; extend or shorten, but it must stay in the future.
/// Not updatable: mint_a / deposit (would need tokens moved in or out of the
/// vault, use refund + make instead), mint_b (changes the whole deal), and the
/// designated taker (keeps the deal the taker agreed to look at).
#[derive(Accounts)]
pub struct Update<'info> {
    pub maker: Signer<'info>,

    #[account(
        mut,
        has_one = maker,
        seeds = [ESCROW_SEED, maker.key().as_ref(), escrow.seed.to_le_bytes().as_ref()],
        bump = escrow.bump
    )]
    pub escrow: Account<'info, Escrow>,
}

impl<'info> Update<'info> {
    pub fn update(&mut self, new_receive: u64, new_deadline: i64) -> Result<()> {
        let now = Clock::get()?.unix_timestamp;

        // Only an open offer can be changed. An expired offer can only be refunded.
        require!(now < self.escrow.deadline, EscrowError::EscrowExpired);
        require!(new_receive > 0, EscrowError::InvalidAmount);
        require!(new_deadline > now, EscrowError::InvalidDeadline);

        self.escrow.receive = new_receive;
        self.escrow.deadline = new_deadline;
        Ok(())
    }
}
