// contracts/pool-constant-product/src/execute.rs

use cosmwasm_std::{
    from_json, to_json_binary, Addr, BankMsg, Coin, Deps, DepsMut, Env, MessageInfo, QueryRequest,
    Response, StdResult, Uint128, WasmQuery,
};
use cw20::Cw20ReceiveMsg;

use crate::error::ContractError;
use crate::msg::{Cw20HookMsg, InstantiateMsg};
use crate::state::{
    PoolConfig, CONTRACT_NAME, CONTRACT_VERSION, POOL_CONFIG, RESERVE_A, RESERVE_B,
};

// Import helpers from other modules for this contract
use crate::calculations::*;
use crate::messaging::*;
use crate::validation::*;
use cw2;
// No need for state::get_ordered_denoms here

// --- Instantiate Handler ---
pub(crate) fn execute_instantiate(
    deps: DepsMut,
    env: Env,
    _info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let factory_addr = deps.api.addr_validate(&msg.factory_addr)?;
    let (denom_a, denom_b) = {
        if msg.denom_a < msg.denom_b {
            (msg.denom_a.clone(), msg.denom_b.clone())
        } else {
            (msg.denom_b.clone(), msg.denom_a.clone())
        }
    };
    RESERVE_A.save(deps.storage, &Uint128::zero())?;
    RESERVE_B.save(deps.storage, &Uint128::zero())?;

    let sub_msg = create_lp_instantiate_submsg(msg.lp_token_code_id, &env, &denom_a, &denom_b)?;

    let cfg = PoolConfig {
        factory_addr,
        denom_a: denom_a.clone(),
        denom_b: denom_b.clone(),
        lp_token_addr: Addr::unchecked(""),
    };
    POOL_CONFIG.save(deps.storage, &cfg)?;
    cw2::set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;

    Ok(Response::new()
        .add_submessage(sub_msg)
        .add_attribute("action", "instantiate_pool_contract")
        .add_attribute("factory", msg.factory_addr)
        .add_attribute("denom_a", denom_a)
        .add_attribute("denom_b", denom_b)
        .add_attribute("lp_token_code_id", msg.lp_token_code_id.to_string()))
}

// --- Execute Handler Implementations ---

pub(crate) fn execute_add_liquidity(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> Result<Response, ContractError> {
    let cfg = POOL_CONFIG.load(deps.storage)?;
    if cfg.lp_token_addr == Addr::unchecked("") {
        return Err(ContractError::NotInitialized {});
    }

    let current_reserve_a = query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_a)?;
    let current_reserve_b = query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_b)?;
    let total_shares = query_cw20_total_supply(deps.as_ref(), &cfg.lp_token_addr)?;

    let (amount_a, amount_b) = validate_and_get_liquidity_funds(&info, &cfg.denom_a, &cfg.denom_b)?;

    let shares_to_mint = if total_shares.is_zero() {
        calculate_initial_lp_shares(amount_a, amount_b)?
    } else {
        let reserve_a_before = current_reserve_a.checked_sub(amount_a)?;
        let reserve_b_before = current_reserve_b.checked_sub(amount_b)?;
        validate_deposit_ratio(amount_a, amount_b, reserve_a_before, reserve_b_before)?;
        calculate_subsequent_lp_shares(
            amount_a,
            amount_b,
            reserve_a_before,
            reserve_b_before,
            total_shares,
        )?
    };

    let mint_msg =
        create_mint_message(&cfg.lp_token_addr, info.sender.to_string(), shares_to_mint)?;

    // TODO: Add event emission
    Ok(Response::new()
        .add_message(mint_msg)
        .add_attribute("action", "add_liquidity")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("denom_a_deposited", amount_a.to_string())
        .add_attribute("denom_b_deposited", amount_b.to_string())
        .add_attribute("shares_minted", shares_to_mint.to_string()))
}

pub(crate) fn execute_swap(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    offer_denom: String,
    min_receive: Uint128,
) -> Result<Response, ContractError> {
    let cfg = POOL_CONFIG.load(deps.storage)?;
    if cfg.lp_token_addr == Addr::unchecked("") {
        return Err(ContractError::NotInitialized {});
    }

    let offer_amount = get_offer_amount(&info, &offer_denom)?;
    let current_reserve_a = query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_a)?;
    let current_reserve_b = query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_b)?;

    let (ask_denom, reserve_in, reserve_out) = if offer_denom == cfg.denom_a {
        (cfg.denom_b.clone(), current_reserve_a, current_reserve_b)
    } else if offer_denom == cfg.denom_b {
        (cfg.denom_a.clone(), current_reserve_b, current_reserve_a)
    } else {
        return Err(ContractError::InvalidLiquidityDenom { denom: offer_denom });
    };

    // TODO: Fee logic needs solidifying. Using placeholders.
    let fee_numerator = 3u64;
    let fee_denominator = 1000u64;

    let output_amount = calculate_swap_output(
        offer_amount,
        reserve_in,
        reserve_out,
        fee_numerator,
        fee_denominator,
    )?;

    if output_amount < min_receive {
        return Err(ContractError::SwapMinimumReceiveViolation {
            output: output_amount,
            min_receive,
        });
    }

    let return_msg = BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: vec![Coin {
            denom: ask_denom.clone(),
            amount: output_amount,
        }],
    };

    // TODO: Add event emission
    Ok(Response::new()
        .add_message(return_msg)
        .add_attribute("action", "swap")
        .add_attribute("sender", info.sender.to_string())
        .add_attribute("offer_denom", offer_denom)
        .add_attribute("ask_denom", ask_denom)
        .add_attribute("offer_amount", offer_amount.to_string())
        .add_attribute("return_amount", output_amount.to_string()))
}

pub(crate) fn execute_cw20_receive(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> Result<Response, ContractError> {
    let cfg = POOL_CONFIG.load(deps.storage)?;
    if info.sender != cfg.lp_token_addr {
        return Err(ContractError::UnauthorizedLpToken {
            expected: cfg.lp_token_addr,
        });
    }

    match from_json(&cw20_msg.msg)? {
        Cw20HookMsg::WithdrawLiquidity {} => {
            if cw20_msg.amount.is_zero() {
                return Err(ContractError::ZeroWithdrawAmount {});
            }

            let current_reserve_a =
                query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_a)?;
            let current_reserve_b =
                query_bank_balance(deps.as_ref(), &env.contract.address, &cfg.denom_b)?;
            let total_shares = query_cw20_total_supply(deps.as_ref(), &cfg.lp_token_addr)?;

            let (return_a, return_b) = calculate_withdraw_amounts(
                cw20_msg.amount,
                current_reserve_a,
                current_reserve_b,
                total_shares,
            )?;

            let burn_msg = create_burn_message(&cfg.lp_token_addr, cw20_msg.amount)?;
            let return_funds_msg = BankMsg::Send {
                to_address: cw20_msg.sender.clone(),
                amount: vec![
                    Coin {
                        denom: cfg.denom_a.clone(),
                        amount: return_a,
                    },
                    Coin {
                        denom: cfg.denom_b.clone(),
                        amount: return_b,
                    },
                ],
            };

            // TODO: Add event emission
            Ok(Response::new()
                .add_message(burn_msg)
                .add_message(return_funds_msg)
                .add_attribute("action", "withdraw_liquidity")
                .add_attribute("sender", cw20_msg.sender) // User receiving funds
                .add_attribute("lp_token_contract", info.sender.to_string()) // LP token burned
                .add_attribute("withdrawn_share", cw20_msg.amount.to_string())
                .add_attribute("return_a", return_a.to_string())
                .add_attribute("return_b", return_b.to_string()))
        }
    }
}

// --- Internal Helpers ---

/// Helper function to query bank balance using query_balance method.
fn query_bank_balance(deps: Deps, contract_addr: &Addr, denom: &str) -> StdResult<Uint128> {
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
