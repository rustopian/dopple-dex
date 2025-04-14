use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{Addr, Uint128};
use cw20::Cw20ReceiveMsg;

/// Message sent by the factory to instantiate this pool logic contract.
#[cw_serde]
pub struct InstantiateMsg {
    pub denom_a: String,
    pub denom_b: String,
    pub lp_token_code_id: u64, // Code ID for the LP token this pool should use
    pub factory_addr: String,  // Address of the factory contract
                               // Potentially add fee info if pool controls fees
}

#[cw_serde]
pub enum ExecuteMsg {
    AddLiquidity {},
    Swap {
        offer_denom: String, // Must match sent funds
        min_receive: Uint128,
    },
    Receive(Cw20ReceiveMsg),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum QueryMsg {
    #[returns(PoolStateResponse)]
    PoolState {},
    // Would be useful to add SimulateSwap query later
    // #[returns(SimulateSwapResponse)]
    // SimulateSwap { offer_amount: Uint128, offer_denom: String },
}

#[cw_serde]
pub struct PoolStateResponse {
    pub denom_a: String,
    pub denom_b: String,
    pub reserve_a: Uint128,
    pub reserve_b: Uint128,
    pub total_lp_shares: Uint128,
    pub lp_token_address: Addr,
}

// Hook message for receiving LP tokens
#[cw_serde]
pub enum Cw20HookMsg {
    WithdrawLiquidity {},
}
