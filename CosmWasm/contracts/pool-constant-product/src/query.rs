use crate::msg::PoolStateResponse;
use crate::state::POOL_CONFIG;
use cosmwasm_std::{
    to_json_binary, Addr, Binary, Deps, Env, QueryRequest, StdResult, Uint128, WasmQuery,
};

// --- Query Handler Implementations ---

pub(crate) fn query_pool_state(deps: Deps, env: Env) -> StdResult<Binary> {
    let cfg = POOL_CONFIG.load(deps.storage)?;

    // Use internal helpers to get current state
    let reserve_a = query_bank_balance(deps, &env.contract.address, &cfg.denom_a)?;
    let reserve_b = query_bank_balance(deps, &env.contract.address, &cfg.denom_b)?;
    let total_shares = query_cw20_total_supply(deps, &cfg.lp_token_addr)?;

    let resp = PoolStateResponse {
        denom_a: cfg.denom_a,
        denom_b: cfg.denom_b,
        reserve_a,
        reserve_b,
        total_lp_shares: total_shares,
        lp_token_address: cfg.lp_token_addr,
    };
    to_json_binary(&resp)
}

// --- Internal Helpers (Copied from execute.rs) ---

/// Helper function to query bank balance using query_balance method.
fn query_bank_balance(deps: Deps, contract_addr: &Addr, denom: &str) -> StdResult<Uint128> {
    use cosmwasm_std::Coin; // Add specific import needed here
    let balance: Coin = deps.querier.query_balance(contract_addr, denom)?;
    Ok(balance.amount)
}

/// Helper function to query CW20 total supply using a WasmQuery.
fn query_cw20_total_supply(deps: Deps, token_addr: &Addr) -> StdResult<Uint128> {
    use cw20::{Cw20QueryMsg, TokenInfoResponse};
    let token_info: TokenInfoResponse =
        deps.querier.query(&QueryRequest::Wasm(WasmQuery::Smart {
            contract_addr: token_addr.to_string(),
            msg: to_json_binary(&Cw20QueryMsg::TokenInfo {})?,
        }))?;
    Ok(token_info.total_supply)
}

// TODO: Add simulate_swap query implementation if needed
