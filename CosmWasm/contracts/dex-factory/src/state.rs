use cosmwasm_schema::cw_serde;
use cosmwasm_std::Addr;
use cw_storage_plus::{Item, Map};

pub const INSTANTIATE_POOL_REPLY_ID: u64 = 1;
pub const CONTRACT_NAME: &str = "crates.io:cw-dex-factory";
pub const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cw_serde]
pub struct Config {
    /// Code ID of the default LP logic contract
    pub default_pool_logic_code_id: u64,
    /// Address with power to update the config
    pub admin: Addr,
}

// Temporary storage for pool key during pool contract instantiation reply
pub type PendingPoolInstanceKey = (String, String, u64);
pub const PENDING_POOL_INSTANCE: Item<PendingPoolInstanceKey> = Item::new("pending_pool_instance");

pub const CONFIG: Item<Config> = Item::new("config");
// Key: (denom_a, denom_b, pool_logic_code_id), Value: Addr of the pool contract instance
pub const POOLS: Map<(String, String, u64), Addr> = Map::new("pools");

/// Returns denoms in a canonical (alphabetical) order.
/// Keeping this here for pool key creation.
pub(crate) fn get_ordered_denoms(denom_a: String, denom_b: String) -> (String, String) {
    if denom_a < denom_b {
        (denom_a, denom_b)
    } else {
        (denom_b, denom_a)
    }
}

// Removed other helpers
