# Dopple DEX

This project implements the Dopple DEX, a decentralized exchange, on two different blockchain platforms: Solana and CosmWasm.

Full LiteSVM and Multitest tests are included.

## Security Matters

Right now, `constant_product_plugin` can be called externally. This is okay, since the plugin doesn't actually control any balances; it only replies to requests for mathematical operations. Still, I'd like to analyze this for any potential exploits.

- **Solana:** Contains the implementation for the Solana blockchain. See the [Solana README](./Solana/README.md) for details.
- **CosmWasm:** Contains the implementation using the CosmWasm framework. See the [CosmWasm README](./CosmWasm/README.md) for details.

We'll keep things minimal but useful:
- Users can create liquidity pools with custom logic. We'll add a constant-product (x * y = k) market maker, like Uniswap v2 and most early AMM DEXes. However, the system will be easily extensible to include other liquidity pool types: meaning that a dedicated, independent plugin contract/program will handle the pool logic.
- Users can deposit liquidity into pools.
- Users can swap from one token to another. Fees taken on swaps go back to the liquidity pool, as usual, growing its value over time (all else being equal).
- Users can withdraw liquidity from pools.

This gives us our 4 main actions right away:
- Create Pool
- Deposit
- Swap
- Withdraw

To ward off any unnecessary details, we'll ignore these:
- Governance or authority over pool creation. Anyone can create pools, but only one pool can exist for any given asset pair with any given logic plugin.
- Locked liquidity, staked liquidity, etc. All liquidity is subject to withdrawal at any time.
- Single-token withdrawal. When LP tokens are burned, they give the user tokens from both sides, in equal measure according to the current balance between pools.
- For CosmWasm, we'll ignore CW20 tokens - most assets of value are Token Factory or IBC assets, which act like native assets. Our Solana DEX, however, will definitely need to support SPL tokens in addition to SOL. We'll still demonstrate plenty of multi-contract interaction on the CosmWasm side, since LP tokens will be their own CW20 contracts.
