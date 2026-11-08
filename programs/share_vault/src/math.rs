use anchor_lang::prelude::*;

use crate::error::VaultError;

/// a * b / c in u128, rounded down. Rounding down always favours the vault
/// (users get slightly fewer shares or tokens, never more than they own).
pub fn mul_div(a: u64, b: u64, c: u64) -> Result<u64> {
    require!(c > 0, VaultError::MathOverflow);
    let out = (a as u128)
        .checked_mul(b as u128)
        .ok_or(VaultError::MathOverflow)?
        / (c as u128);
    u64::try_from(out).map_err(|_| error!(VaultError::MathOverflow))
}

/// Total value of the vault in asset units. Assumption for this demo: one
/// position token is worth one asset token (a receipt at par). A real vault
/// would use the market's exchange rate or an oracle here. Only deposit and the
/// cash withdraw use this price; redeem_in_kind never does.
pub fn total_value(idle: u64, position: u64) -> Result<u64> {
    idle.checked_add(position)
        .ok_or_else(|| error!(VaultError::MathOverflow))
}
