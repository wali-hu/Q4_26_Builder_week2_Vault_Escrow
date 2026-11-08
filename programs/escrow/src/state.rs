use anchor_lang::prelude::*;

#[account]
#[derive(InitSpace)]
pub struct Escrow {
    /// Lets one maker run many offers at once: part of the PDA seeds.
    pub seed: u64,
    pub maker: Pubkey,
    /// Token the maker deposits (held in the vault).
    pub mint_a: Pubkey,
    /// Token the maker wants back.
    pub mint_b: Pubkey,
    /// Amount of mint_b the maker wants, in base units. Updatable.
    pub receive: u64,
    /// Unix timestamp after which take is refused. Updatable.
    pub deadline: i64,
    /// If set, only this wallet may take the offer (the signature condition).
    pub taker: Option<Pubkey>,
    pub bump: u8,
}
