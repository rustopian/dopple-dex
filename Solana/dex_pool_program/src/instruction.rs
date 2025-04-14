#![allow(clippy::doc_lazy_continuation)]
use borsh::{BorshDeserialize, BorshSerialize};

/// Defines the instructions available in the Pool program.
#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PoolInstruction {
    /// Initializes a new pool.
    /// Creates the pool state account, vaults, and LP mint.
    ///
    /// Accounts (expected):
    /// 0. [signer] payer: Account funding the new pool
    /// 1. [writable] pool state PDA: Derived from sorted mints + plugin addresses
    /// 2. [writable] vault A: Token account for token A reserves
    /// 3. [writable] vault B: Token account for token B reserves
    /// 4. [writable] LP mint: Mint account for the pool's liquidity provider tokens
    /// 5. [read]   token mint A: Mint of token A
    /// 6. [read]   token mint B: Mint of token B
    /// 7. [read]   plugin program: The executable plugin program ID
    /// 8. [writable] plugin state: The state account for the plugin program
    /// 9. [read]   system_program: Solana System Program
    /// 10. [read]  token_program: SPL Token Program
    /// 11. [read]  rent sysvar: Solana Rent Sysvar
    InitializePool,

    /// Adds liquidity to the pool.
    /// Transfers tokens A and B from the user to the vaults and mints LP tokens to the user.
    ///
    /// Accounts:
    /// 0. [signer] user: The user adding liquidity
    /// 1. [writable] pool state: The pool's state account
    /// 2. [writable] vault A: Pool's token A vault
    /// 3. [writable] vault B: Pool's token B vault
    /// 4. [writable] LP mint: Pool's LP mint account
    /// 5. [writable] user token A: User's source token A account
    /// 6. [writable] user token B: User's source token B account
    /// 7. [writable] user LP: User's destination LP token account
    /// 8. [read]   token_program: SPL Token Program
    /// 9. [read]   plugin program: The executable plugin program ID
    /// 10.[writable] plugin state: The state account for the plugin program
    AddLiquidity {
        /// Max amount of token A to deposit
        amount_a: u64,
        /// Max amount of token B to deposit
        amount_b: u64,
    },

    /// Removes liquidity from the pool.
    /// Burns user's LP tokens and transfers tokens A and B from the vaults back to the user.
    ///
    /// Accounts:
    /// 0. [signer] user: The user removing liquidity
    /// 1. [writable] pool state: The pool's state account
    /// 2. [writable] vault A: Pool's token A vault
    /// 3. [writable] vault B: Pool's token B vault
    /// 4. [writable] LP mint: Pool's LP mint account
    /// 5. [writable] user token A: User's destination token A account
    /// 6. [writable] user token B: User's destination token B account
    /// 7. [writable] user LP: User's source LP token account (to burn from)
    /// 8. [read]   token_program: SPL Token Program
    /// 9. [read]   plugin program: The executable plugin program ID
    /// 10.[writable] plugin state: The state account for the plugin program
    RemoveLiquidity {
        /// Amount of LP tokens to burn
        amount_lp: u64,
    },

    /// Swaps one token for another in the pool.
    /// Transfers the input token from the user to the corresponding vault and the output token from the other vault to the user.
    ///
    /// Accounts:
    /// 0. [signer] user: The user performing the swap
    /// 1. [writable] pool state: The pool's state account
    /// 2. [writable] vault A: Pool's token A vault
    /// 3. [writable] vault B: Pool's token B vault
    /// 4. [writable] user src token: User's source token account (sending to pool)
    /// 5. [writable] user dst token: User's destination token account (receiving from pool)
    /// 6. [read]   token_program: SPL Token Program
    /// 7. [read]   plugin program: The executable plugin program ID
    /// 8. [writable] plugin state: The state account for the plugin program
    Swap {
        /// Amount of the input token to swap
        amount_in: u64,
        /// Minimum amount of the output token the user must receive (slippage protection)
        min_out: u64,
    },
}
