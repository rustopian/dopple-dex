use crate::error::ContractError;
use crate::state::{INSTANTIATE_POOL_REPLY_ID, PENDING_POOL_INSTANCE, POOLS};
use cosmwasm_std::{DepsMut, Reply, Response, StdError};
use cw_utils::parse_instantiate_response_data;

pub fn handle_lp_instantiate_reply(deps: DepsMut, msg: Reply) -> Result<Response, ContractError> {
    if msg.id != INSTANTIATE_POOL_REPLY_ID {
        return Err(ContractError::UnknownReplyId { id: msg.id });
    }

    let result = msg.result.into_result().map_err(StdError::generic_err)?;
    #[allow(deprecated)]
    let data = result.data.ok_or(ContractError::MissingReplyData {})?;
    let res = parse_instantiate_response_data(&data)?;

    let pool_contract_addr = deps.api.addr_validate(&res.contract_address)?;

    let pool_key = PENDING_POOL_INSTANCE.load(deps.storage)?;

    POOLS.save(deps.storage, pool_key.clone(), &pool_contract_addr)?;

    PENDING_POOL_INSTANCE.remove(deps.storage);

    Ok(Response::new()
        .add_attribute("action", "pool_instance_created")
        .add_attribute("pool_contract_address", pool_contract_addr.to_string())
        .add_attribute("denom_a", pool_key.0)
        .add_attribute("denom_b", pool_key.1)
        .add_attribute("pool_logic_code_id", pool_key.2.to_string()))
}
