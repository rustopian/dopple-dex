use crate::error::ContractError;
use crate::msg::PoolContractInstantiateMsg;
use crate::state::{
    get_ordered_denoms as get_ordered_denoms_state, CONFIG, INSTANTIATE_POOL_REPLY_ID,
    PENDING_POOL_INSTANCE, POOLS,
};
use cosmwasm_std::{to_json_binary, DepsMut, Env, MessageInfo, Response, SubMsg, WasmMsg};

// --- Execute Handlers ---

pub(crate) fn execute_create_pool(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    denom_a: String,
    denom_b: String,
    pool_logic_code_id: u64,
) -> Result<Response, ContractError> {
    if denom_a == denom_b {
        return Err(ContractError::IdenticalDenoms {});
    }
    let pool_key_denoms = get_ordered_denoms_state(denom_a.clone(), denom_b.clone());
    let cfg = CONFIG.load(deps.storage)?;
    let pool_key = (
        pool_key_denoms.0.clone(),
        pool_key_denoms.1.clone(),
        pool_logic_code_id,
    );

    if POOLS.may_load(deps.storage, pool_key.clone())?.is_some() {
        return Err(ContractError::PoolAlreadyExists {
            denom1: pool_key.0,
            denom2: pool_key.1,
        });
    }
    if PENDING_POOL_INSTANCE.may_load(deps.storage)?.is_some() {
        return Err(ContractError::PoolCreationPending {});
    }

    if !info.funds.is_empty() {
        return Err(ContractError::FundsSentOnCreatePool {});
    }

    let instantiate_pool_msg = PoolContractInstantiateMsg {
        denom_a: pool_key.0.clone(),
        denom_b: pool_key.1.clone(),
        lp_token_code_id: cfg.default_pool_logic_code_id,
        factory_addr: env.contract.address.clone(),
    };

    let submsg = SubMsg::reply_on_success(
        WasmMsg::Instantiate {
            admin: Some(env.contract.address.to_string()),
            code_id: pool_logic_code_id,
            msg: to_json_binary(&instantiate_pool_msg)?,
            funds: vec![],
            label: format!(
                "DEX Pool-{}-{} (Logic {})",
                pool_key.0, pool_key.1, pool_logic_code_id
            ),
        },
        INSTANTIATE_POOL_REPLY_ID,
    );

    PENDING_POOL_INSTANCE.save(deps.storage, &pool_key)?;

    Ok(Response::new()
        .add_submessage(submsg)
        .add_attribute("action", "create_pool_instance")
        .add_attribute("pool_logic_code_id", pool_logic_code_id.to_string())
        .add_attribute("denom_a", pool_key.0)
        .add_attribute("denom_b", pool_key.1))
}

// --- Admin Handlers ---

pub(crate) fn execute_register_pool_type(
    deps: DepsMut,
    info: MessageInfo,
    pool_logic_code_id: u64,
) -> Result<Response, ContractError> {
    let cfg = CONFIG.load(deps.storage)?;
    if cfg.admin != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    Ok(Response::new()
        .add_attribute("action", "register_pool_type")
        .add_attribute("code_id", pool_logic_code_id.to_string()))
}

pub(crate) fn execute_update_admin(
    deps: DepsMut,
    info: MessageInfo,
    new_admin: Option<String>,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    if cfg.admin != info.sender {
        return Err(ContractError::Unauthorized {});
    }

    let final_new_admin_addr = match new_admin {
        Some(admin_str) => Some(deps.api.addr_validate(&admin_str)?),
        None => None,
    };

    cfg.admin = final_new_admin_addr.ok_or(ContractError::AdminCannotBeNone {})?;

    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::new()
        .add_attribute("action", "update_admin")
        .add_attribute("new_admin", cfg.admin.to_string()))
}

pub(crate) fn execute_update_default_pool_logic_code_id(
    deps: DepsMut,
    info: MessageInfo,
    new_code_id: u64,
) -> Result<Response, ContractError> {
    let mut cfg = CONFIG.load(deps.storage)?;
    if cfg.admin != info.sender {
        return Err(ContractError::Unauthorized {});
    }
    cfg.default_pool_logic_code_id = new_code_id;
    CONFIG.save(deps.storage, &cfg)?;
    Ok(Response::new()
        .add_attribute("action", "update_default_pool_logic_code_id")
        .add_attribute("new_code_id", new_code_id.to_string()))
}
