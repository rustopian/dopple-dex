use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::Addr;

use crate::state::Config;

/// Instantiate message for the Factory contract.
#[cw_serde]
pub struct InstantiateMsg {
    pub default_pool_logic_code_id: u64,
    pub admin: String,
}

/// Execute messages for the Factory contract.
#[cw_serde]
pub enum ExecuteMsg {
    /// Create a new liquidity pool instance using a specific pool logic contract.
    CreatePool {
        denom_a: String,
        denom_b: String,
        pool_logic_code_id: u64,
    },
    /// Allows admin to register a new pool logic contract code ID.
    RegisterPoolType { pool_logic_code_id: u64 },
    /// Update admin.
    UpdateAdmin { new_admin: Option<String> },
    /// Update default LP token code ID.
    UpdateDefaultLpCodeId { new_code_id: u64 },
}

#[cw_serde]
pub struct MigrateMsg {}

/// Message sent by the factory to instantiate a new pool logic contract.
#[cw_serde]
pub struct PoolContractInstantiateMsg {
    pub denom_a: String,
    pub denom_b: String,
    pub lp_token_code_id: u64,
    pub factory_addr: Addr,
}

/// Factory Query Messages
#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    /// Get the address of a specific pool instance.
    #[returns(Addr)]
    PoolAddress {
        denom_a: String,
        denom_b: String,
        pool_logic_code_id: u64,
    },
    /// Get the factory configuration.
    #[returns(Config)]
    Config {},
}