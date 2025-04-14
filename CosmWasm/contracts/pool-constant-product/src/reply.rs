use cosmwasm_std::{Addr, DepsMut, Reply, Response, StdError, StdResult};
use cw_utils::parse_instantiate_response_data;

use crate::error::ContractError;
use crate::state::{INSTANTIATE_LP_REPLY_ID, POOL_CONFIG};

pub fn handle_lp_instantiate_reply(deps: DepsMut, msg: Reply) -> Result<Response, ContractError> {
    if msg.id != INSTANTIATE_LP_REPLY_ID {
        return Err(ContractError::UnknownReplyId { id: msg.id });
    }

    let result = msg.result.into_result().map_err(StdError::generic_err)?;
    #[allow(deprecated)]
    let data = result.data.ok_or(ContractError::MissingReplyData {})?;
    let res = parse_instantiate_response_data(&data)?;

    println!(
        "[reply] Received contract_address in reply data: {}",
        res.contract_address
    );
    #[cfg(not(test))]
    let lp_token_addr = deps.api.addr_validate(&res.contract_address)?;
    #[cfg(test)]
    let lp_token_addr = Addr::unchecked(&res.contract_address);

    // Update config with the LP token address
    POOL_CONFIG.update(deps.storage, |mut cfg| -> StdResult<_> {
        // Safety check: ensure lp_token_addr is not already set
        // This prevents potential issues if reply is somehow triggered twice
        if cfg.lp_token_addr != Addr::unchecked("") {
            return Err(StdError::generic_err("LP token address already set"));
        }
        cfg.lp_token_addr = lp_token_addr.clone();
        Ok(cfg)
    })?;

    Ok(Response::new()
        .add_attribute("action", "lp_token_instantiated")
        .add_attribute("lp_token_address", lp_token_addr))
}
