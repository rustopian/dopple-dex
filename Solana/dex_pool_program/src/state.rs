use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::pubkey::Pubkey;

/// The main state account for a liquidity pool.
/// Convenient for retrieving pool information.
///
/// It stores references to:
/// - Mints for the two assets being pooled and the LP token mint.
/// - Vaults (token accounts) that hold the pool's reserves of each asset.
/// - Information about the associated pricing plugin.
/// - The total supply of LP tokens currently minted.
/// - The bump seed used for the pool's PDA.
#[derive(BorshSerialize, BorshDeserialize, Debug, PartialEq)]
#[repr(C)]
pub struct PoolState {
    /// Mint address of the first token (Token A).
    pub token_mint_a: Pubkey,
    /// Mint address of the second token (Token B).
    pub token_mint_b: Pubkey,
    /// Token account holding the pool's reserves of Token A.
    pub vault_a: Pubkey,
    /// Token account holding the pool's reserves of Token B.
    pub vault_b: Pubkey,
    /// Mint address for the liquidity provider (LP) tokens.
    pub lp_mint: Pubkey,
    /// The total amount of LP tokens currently minted.
    pub total_lp_supply: u64,
    /// The bump seed used to derive the pool state's PDA.
    pub bump: u8,

    // Plugin references
    /// The program ID of the associated pricing plugin.
    pub plugin_program_id: Pubkey,
    /// The account address of the plugin's specific state for this pool.
    pub plugin_state_pubkey: Pubkey,
}
