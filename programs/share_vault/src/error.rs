use anchor_lang::prelude::*;

#[error_code]
pub enum VaultError {
    // 6000
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    // 6001
    #[msg("Not enough idle liquidity for a cash withdraw: use redeem_in_kind")]
    NotEnoughLiquidity,
    // 6002
    #[msg("Amount too small: it would round down to zero")]
    RoundsToZero,
    // 6003
    #[msg("Math overflow")]
    MathOverflow,
}
