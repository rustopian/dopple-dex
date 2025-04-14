use crate::execute::{execute_add_liquidity, execute_cw20_receive, execute_swap};
use crate::query::query_pool_state;
use crate::reply::handle_lp_instantiate_reply;
use cosmwasm_std::{
    entry_point, Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult,
};

use crate::error::ContractError;
use crate::msg::{ExecuteMsg, InstantiateMsg, QueryMsg};

// --- Entry Points ---

#[entry_point]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    crate::execute::execute_instantiate(deps, env, _info, msg)
}

#[entry_point]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::AddLiquidity {} => execute_add_liquidity(deps, env, info),
        ExecuteMsg::Swap {
            offer_denom,
            min_receive,
        } => execute_swap(deps, env, info, offer_denom, min_receive),
        ExecuteMsg::Receive(cw20_msg) => execute_cw20_receive(deps, env, info, cw20_msg),
    }
}

#[entry_point]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::PoolState {} => query_pool_state(deps, env),
    }
}

#[entry_point]
pub fn reply(deps: DepsMut, _env: Env, msg: Reply) -> Result<Response, ContractError> {
    handle_lp_instantiate_reply(deps, msg)
}
