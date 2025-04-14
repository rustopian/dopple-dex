use crate::error::ContractError;
use crate::execute::{
    execute_create_pool, execute_register_pool_type, execute_update_admin,
    execute_update_default_pool_logic_code_id,
};
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};
use crate::query::{query_config, query_pool_address};
use crate::reply::handle_lp_instantiate_reply;
use crate::state::{Config, CONFIG, CONTRACT_NAME, CONTRACT_VERSION};
use cosmwasm_std::{
    entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};

// --- Entry Points ---

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    let admin_addr = deps.api.addr_validate(&msg.admin)?;

    let cfg = Config {
        default_pool_logic_code_id: msg.default_pool_logic_code_id,
        admin: admin_addr.clone(),
    };

    CONFIG.save(deps.storage, &cfg)?;
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    Ok(Response::new()
        .add_attribute("action", "instantiate")
        .add_attribute("admin", admin_addr.to_string())
        .add_attribute(
            "default_pool_logic_code_id",
            cfg.default_pool_logic_code_id.to_string(),
        ))
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::CreatePool {
            denom_a,
            denom_b,
            pool_logic_code_id,
        } => execute_create_pool(deps, env, info, denom_a, denom_b, pool_logic_code_id),
        ExecuteMsg::RegisterPoolType { pool_logic_code_id } => {
            execute_register_pool_type(deps, info, pool_logic_code_id)
        }
        ExecuteMsg::UpdateAdmin { new_admin } => execute_update_admin(deps, info, new_admin),
        ExecuteMsg::UpdateDefaultLpCodeId { new_code_id } => {
            execute_update_default_pool_logic_code_id(deps, info, new_code_id)
        }
    }
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    handle_lp_instantiate_reply(deps, msg)
}

#[entry_point]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::PoolAddress {
            denom_a,
            denom_b,
            pool_logic_code_id,
        } => query_pool_address(deps, denom_a, denom_b, pool_logic_code_id),
        QueryMsg::Config {} => query_config(deps),
    }
}
