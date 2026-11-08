use anchor_lang::prelude::*;

#[error_code]
pub enum ErrorCode {
    // 6000
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    // 6001
    #[msg("Not enough lamports in the vault")]
    InsufficientFunds,
    // 6002
    #[msg("Withdraw would leave the vault above 0 but below its rent-exempt minimum")]
    WouldBreakRent,
}
