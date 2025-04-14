use crate::state::{get_ordered_denoms as get_ordered_denoms_state, Config, CONFIG, POOLS};
use cosmwasm_std::{to_json_binary, Binary, Deps, StdResult};

// --- Query Handlers ---

pub(crate) fn query_pool_address(
    deps: Deps,
    denom_a: String,
    denom_b: String,
    pool_logic_code_id: u64,
) -> StdResult<Binary> {
    let key_denoms = get_ordered_denoms_state(denom_a, denom_b);
    let key = (key_denoms.0, key_denoms.1, pool_logic_code_id);
    let pool_addr = POOLS.load(deps.storage, key)?;
    to_json_binary(&pool_addr)
}

pub(crate) fn query_config(deps: Deps) -> StdResult<Binary> {
    let cfg = CONFIG.load(deps.storage)?;
    let resp = Config {
        admin: cfg.admin,
        default_pool_logic_code_id: cfg.default_pool_logic_code_id,
    };
    to_json_binary(&resp)
}

// Removed old query_pool implementation
