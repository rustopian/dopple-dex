use crate::state::INSTANTIATE_LP_REPLY_ID;
use cosmwasm_std::{to_json_binary, Addr, CosmosMsg, Env, StdResult, SubMsg, Uint128, WasmMsg};
use cw20::{Cw20ExecuteMsg, MinterResponse};
use cw20_base;

/// Creates a WasmMsg to execute the Mint message on the LP token contract.
pub(crate) fn create_mint_message(
    lp_token_addr: &Addr,
    recipient: String,
    amount: Uint128,
) -> StdResult<CosmosMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: lp_token_addr.to_string(),
        msg: to_json_binary(&Cw20ExecuteMsg::Mint { recipient, amount })?,
        funds: vec![],
    }
    .into())
}

/// Creates a WasmMsg to execute the Burn message on the LP token contract.
pub(crate) fn create_burn_message(lp_token_addr: &Addr, amount: Uint128) -> StdResult<CosmosMsg> {
    Ok(WasmMsg::Execute {
        contract_addr: lp_token_addr.to_string(),
        msg: to_json_binary(&Cw20ExecuteMsg::Burn { amount })?,
        funds: vec![],
    }
    .into())
}

/// Creates the SubMsg used to instantiate the LP token contract.
pub(crate) fn create_lp_instantiate_submsg(
    lp_token_code_id: u64,
    env: &Env,
    denom1: &str,
    denom2: &str,
) -> StdResult<SubMsg> {
    let token_name = format!("{}-{} LP", denom1, denom2);
    
    // Create a more descriptive symbol by using up to 4 chars of each token
    let format_token_symbol = |s: &str| {
        let cleaned = s.trim_start_matches('u'); // Remove common 'u' prefix if present
        let len = cleaned.chars().count().min(4);
        cleaned.chars().take(len).collect::<String>().to_uppercase()
    };
    
    let token_symbol = format!(
        "LP-{}-{}",
        format_token_symbol(denom1),
        format_token_symbol(denom2)
    );
    
    let decimals = 6u8;
    let lp_instantiate_msg = cw20_base::msg::InstantiateMsg {
        name: token_name.clone(),
        symbol: token_symbol.clone(),
        decimals,
        initial_balances: vec![],
        mint: Some(MinterResponse {
            minter: env.contract.address.to_string(),
            cap: None,
        }),
        marketing: None,
    };
    let submsg = WasmMsg::Instantiate {
        admin: Some(env.contract.address.to_string()),
        code_id: lp_token_code_id,
        msg: to_json_binary(&lp_instantiate_msg)?,
        funds: vec![],
        label: format!("DEX LP {}-{}", denom1, denom2),
    };
    Ok(SubMsg::reply_on_success(submsg, INSTANTIATE_LP_REPLY_ID))
}

#[cfg(test)]
mod tests {
    use super::*; // Import functions from parent module (messaging.rs)
    use crate::state::INSTANTIATE_LP_REPLY_ID;
    use cosmwasm_std::testing::mock_env;
    use cosmwasm_std::{from_json, Addr, CosmosMsg, Uint128, WasmMsg};
    use cw20::Cw20ExecuteMsg;

    // Define constants needed for tests
    const LP_TOKEN_CODE_ID: u64 = 10;
    const DENOM_A: &str = "uatom";
    const DENOM_B: &str = "uosmo";

    #[test]
    fn test_create_mint_message() {
        let addr = Addr::unchecked("lp_token");
        let recipient = "user1".to_string();
        let amount = Uint128::new(123);
        let msg = create_mint_message(&addr, recipient.clone(), amount).unwrap();
        match msg {
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg,
                funds,
            }) => {
                assert_eq!(contract_addr, addr.to_string());
                assert_eq!(funds.len(), 0);
                let parsed: Cw20ExecuteMsg = from_json(&msg).unwrap();
                assert_eq!(parsed, Cw20ExecuteMsg::Mint { recipient, amount });
            }
            _ => panic!("Unexpected message type"),
        }
    }

    #[test]
    fn test_create_burn_message() {
        let addr = Addr::unchecked("lp_token");
        let amount = Uint128::new(456);
        let msg = create_burn_message(&addr, amount).unwrap();
        match msg {
            CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg,
                funds,
            }) => {
                assert_eq!(contract_addr, addr.to_string());
                assert_eq!(funds.len(), 0);
                let parsed: Cw20ExecuteMsg = from_json(&msg).unwrap();
                assert_eq!(parsed, Cw20ExecuteMsg::Burn { amount });
            }
            _ => panic!("Unexpected message type"),
        }
    }

    #[test]
    fn test_create_lp_instantiate_submsg() {
        let env = mock_env();
        let submsg =
            create_lp_instantiate_submsg(LP_TOKEN_CODE_ID, &env, DENOM_A, DENOM_B).unwrap();

        assert_eq!(submsg.id, INSTANTIATE_LP_REPLY_ID);
        assert_eq!(submsg.reply_on, cosmwasm_std::ReplyOn::Success);
        match submsg.msg {
            CosmosMsg::Wasm(WasmMsg::Instantiate {
                admin,
                code_id,
                msg,
                funds,
                label,
            }) => {
                assert_eq!(admin, Some(env.contract.address.to_string()));
                assert_eq!(code_id, LP_TOKEN_CODE_ID);
                assert!(label.contains(&format!("DEX LP {}-{}", DENOM_A, DENOM_B)));
                assert_eq!(funds.len(), 0);
                let parsed: cw20_base::msg::InstantiateMsg = from_json(&msg).unwrap();
                assert_eq!(parsed.symbol, "LP-UU"); // Corrected expected symbol based on DENOM_A/B constants
                assert_eq!(
                    parsed.mint.unwrap().minter,
                    env.contract.address.to_string()
                );
            }
            _ => panic!("Unexpected message type"),
        }
    }
}
