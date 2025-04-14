use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Uint128};
use cw_storage_plus::Item;

#[cw_serde]
pub struct PoolConfig {
    pub factory_addr: Addr,
    pub denom_a: String,
    pub denom_b: String,
    pub lp_token_addr: Addr,
}

// Store reserves directly
pub const RESERVE_A: Item<Uint128> = Item::new("reserve_a");
pub const RESERVE_B: Item<Uint128> = Item::new("reserve_b");
pub const POOL_CONFIG: Item<PoolConfig> = Item::new("pool_config");

pub const INSTANTIATE_LP_REPLY_ID: u64 = 1; // Local reply ID for this contract

// Contract name and version (optional, but good practice)
pub const CONTRACT_NAME: &str = "crates.io:cw-dex-pool-constant-product";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");
