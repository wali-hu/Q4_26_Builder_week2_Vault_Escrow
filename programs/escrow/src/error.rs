use anchor_lang::prelude::*;

#[error_code]
pub enum EscrowError {
    // 6000
    #[msg("Amount must be greater than zero")]
    InvalidAmount,
    // 6001
    #[msg("Deadline must be in the future")]
    InvalidDeadline,
    // 6002
    #[msg("The offer has expired")]
    EscrowExpired,
    // 6003
    #[msg("Only the designated taker can take this offer")]
    NotDesignatedTaker,
    // 6004
    #[msg("The offer terms changed since the taker read them")]
    TermsChanged,
    // 6005
    #[msg("The maker cannot take their own offer")]
    MakerCannotTake,
    // 6006
    #[msg("Only the maker can refund before the deadline")]
    OnlyMakerBeforeDeadline,
}
