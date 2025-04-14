use cosmwasm_std::{to_json_binary, Addr, Api, Coin, Uint128};
use cw20::{BalanceResponse, TokenInfoResponse};
use cw_multi_test::{App, BankSudo, Contract, ContractWrapper, Executor};
use dex_factory::msg as FactoryMsg;
use pool_constant_product::msg as PoolMsg;
use pool_constant_product::msg::{Cw20HookMsg, PoolStateResponse};

const TOKEN_A: &str = "tokenA";
const TOKEN_B: &str = "tokenB";

// Helper to create contract wrapper for the Factory contract
fn factory_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let contract = ContractWrapper::new(
        dex_factory::contract::execute,
        dex_factory::contract::instantiate,
        dex_factory::contract::query,
    )
    .with_reply(dex_factory::contract::reply);
    Box::new(contract)
}

// Helper to create contract wrapper for the Pool contract
fn pool_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let contract = ContractWrapper::new(
        pool_constant_product::contract::execute,
        pool_constant_product::contract::instantiate,
        pool_constant_product::contract::query,
    )
    .with_reply(pool_constant_product::contract::reply);
    Box::new(contract)
}

// Use cw20-base's contract for LP tokens
fn cw20_contract() -> Box<dyn Contract<cosmwasm_std::Empty>> {
    let contract = ContractWrapper::new(
        cw20_base::contract::execute,
        cw20_base::contract::instantiate,
        cw20_base::contract::query,
    );
    Box::new(contract)
}

/// Sets up app, users, balances, and instantiates DEX Factory
/// Returns: `(App, Factory Address, Factory Code ID, Pool Code ID, Owner Addr, User1 Addr, User2 Addr)`
fn setup_app() -> (App, Addr, u64, u64, Addr, Addr, Addr) {
    let mut app = App::default();
    let cw20_code_id = app.store_code(cw20_contract());
    let factory_code_id = app.store_code(factory_contract());
    let pool_code_id = app.store_code(pool_contract()); // Store pool contract code

    let owner = app.api().addr_make("owner");
    let user1 = app.api().addr_make("user1");
    let user2 = app.api().addr_make("user2");

    // Give users initial balances
    app.sudo(cw_multi_test::SudoMsg::Bank(BankSudo::Mint {
        to_address: user1.to_string(),
        amount: vec![
            Coin {
                denom: TOKEN_A.into(),
                amount: Uint128::new(1_000_000),
            },
            Coin {
                denom: TOKEN_B.into(),
                amount: Uint128::new(1_000_000),
            },
        ],
    }))
    .unwrap();
    app.sudo(cw_multi_test::SudoMsg::Bank(BankSudo::Mint {
        to_address: user2.to_string(),
        amount: vec![
            Coin {
                denom: TOKEN_A.into(),
                amount: Uint128::new(1_000_000),
            },
            Coin {
                denom: TOKEN_B.into(),
                amount: Uint128::new(1_000_000),
            },
        ],
    }))
    .unwrap();

    // Instantiate DEX Factory contract
    let factory_addr = app
        .instantiate_contract(
            factory_code_id,
            owner.clone(),
            &FactoryMsg::InstantiateMsg {
                // Corrected fields for factory instantiation
                default_pool_logic_code_id: cw20_code_id,
                admin: owner.to_string(),
            },
            &[],
            "DexFactoryContract",
            None,
        )
        .unwrap();

    (
        app,
        factory_addr,
        factory_code_id,
        pool_code_id,
        owner,
        user1,
        user2,
    )
}

/// Creates a basic A/B pool via the Factory and provides initial liquidity
/// Returns: `(Pool Address, LP Token Address)`
fn create_basic_pool(
    app: &mut App,
    factory_addr: &Addr,
    pool_code_id: u64,
    user1: &Addr,
) -> (Addr, Addr) {
    // Step 1: Create the pool structure via the factory
    let create_msg = FactoryMsg::ExecuteMsg::CreatePool {
        pool_logic_code_id: pool_code_id,
        denom_a: TOKEN_A.to_string(),
        denom_b: TOKEN_B.to_string(),
    };
    let res_create = app
        .execute_contract(user1.clone(), factory_addr.clone(), &create_msg, &[])
        .unwrap();

    // Find the pool contract address and LP token address from the create_pool response
    // Use the correct attribute keys identified from the debug output
    let pool_addr_str = res_create
        .events
        .iter()
        .find_map(|e| {
            e.attributes
                .iter()
                .find(|a| a.key == "pool_contract_address")
        }) // Correct key
        .map(|a| a.value.clone())
        .expect("Pool address (pool_contract_address) not found in create_pool response events");
    let lp_token_addr_str = res_create
        .events
        .iter()
        .find_map(|e| e.attributes.iter().find(|a| a.key == "lp_token_address"))
        .map(|a| a.value.clone())
        .expect("LP token address (lp_token_address) not found in create_pool response events");

    let pool_addr = app.api().addr_validate(&pool_addr_str).unwrap();
    let lp_token_addr = app.api().addr_validate(&lp_token_addr_str).unwrap();

    // Step 2: Provide initial liquidity directly to the new pool contract
    let initial_a = Uint128::new(100_000);
    let initial_b = Uint128::new(200_000);
    // AddLiquidity in pool takes no args, amounts from funds
    let provide_msg = PoolMsg::ExecuteMsg::AddLiquidity {};

    app.execute_contract(
        user1.clone(),
        pool_addr.clone(),
        &provide_msg,
        &[
            cosmwasm_std::coin(initial_a.u128(), TOKEN_A),
            cosmwasm_std::coin(initial_b.u128(), TOKEN_B),
        ],
    )
    .unwrap();

    (pool_addr, lp_token_addr)
}

#[test]
fn test_full_flow_cosmwasm() {
    let (mut app, factory_addr, _factory_code_id, pool_code_id, _owner, user1, user2) = setup_app();
    let (pool_addr, lp_token_addr) =
        create_basic_pool(&mut app, &factory_addr, pool_code_id, &user1);
    let initial_a = Uint128::new(100_000);
    let initial_b = Uint128::new(200_000);

    // --- Assert initial state (Query the pool contract) ---
    let pool_resp: PoolStateResponse = app
        .wrap()
        .query_wasm_smart(
            pool_addr.clone(), // Query the pool directly
            &PoolMsg::QueryMsg::PoolState {},
        )
        .unwrap();
    // Check pool contract balances (should match initial liquidity)
    let pool_balance_a = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let pool_balance_b = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_B)
        .unwrap()
        .amount;
    assert_eq!(pool_balance_a, initial_a);
    assert_eq!(pool_balance_b, initial_b);
    // Query LP token total supply
    let total_supply: TokenInfoResponse = app
        .wrap()
        .query_wasm_smart(lp_token_addr.clone(), &cw20::Cw20QueryMsg::TokenInfo {})
        .unwrap();
    assert!(total_supply.total_supply > Uint128::zero());
    // Assertions based on PoolStateResponse fields
    assert_eq!(pool_resp.denom_a, TOKEN_A);
    assert_eq!(pool_resp.reserve_a, initial_a);
    assert_eq!(pool_resp.denom_b, TOKEN_B);
    assert_eq!(pool_resp.reserve_b, initial_b);
    // Query user1's LP balance
    let lp_balance: BalanceResponse = app
        .wrap()
        .query_wasm_smart(
            lp_token_addr.clone(),
            &cw20::Cw20QueryMsg::Balance {
                address: user1.to_string(),
            },
        )
        .unwrap();
    assert_eq!(lp_balance.balance, total_supply.total_supply);

    // --- Add liquidity by user2 (Execute on the pool contract) ---
    let add_msg = PoolMsg::ExecuteMsg::AddLiquidity {};
    let add_a = Uint128::new(50_000);
    let add_b = Uint128::new(100_000);
    let _res2 = app
        .execute_contract(
            user2.clone(),
            pool_addr.clone(),
            &add_msg,
            &[
                Coin {
                    denom: TOKEN_A.into(),
                    amount: add_a,
                },
                Coin {
                    denom: TOKEN_B.into(),
                    amount: add_b,
                },
            ],
        )
        .unwrap();

    // Assert state after add liquidity (Query the pool contract)
    let pool_balance_a_after = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let pool_balance_b_after = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_B)
        .unwrap()
        .amount;
    assert_eq!(pool_balance_a_after, initial_a + add_a);
    assert_eq!(pool_balance_b_after, initial_b + add_b);
    // Query new total supply
    let total_supply_after: TokenInfoResponse = app
        .wrap()
        .query_wasm_smart(lp_token_addr.clone(), &cw20::Cw20QueryMsg::TokenInfo {})
        .unwrap();
    let expected_shares_user2 = total_supply_after.total_supply - total_supply.total_supply;
    // Query user2's LP balance
    let lp_balance2: BalanceResponse = app
        .wrap()
        .query_wasm_smart(
            lp_token_addr.clone(),
            &cw20::Cw20QueryMsg::Balance {
                address: user2.to_string(),
            },
        )
        .unwrap();
    assert_eq!(lp_balance2.balance, expected_shares_user2);

    // --- Perform a swap by user2 (Execute on the pool contract) ---
    let swap_msg = PoolMsg::ExecuteMsg::Swap {
        offer_denom: TOKEN_A.into(),
        // ask_denom is inferred by the pool
        min_receive: Uint128::new(1),
    };
    let offer_amount = Uint128::new(10_000);
    let balance_user2_before = app
        .wrap()
        .query_balance(user2.clone(), TOKEN_B)
        .unwrap()
        .amount;
    app.execute_contract(
        user2.clone(),
        pool_addr.clone(),
        &swap_msg,
        &[Coin {
            denom: TOKEN_A.into(),
            amount: offer_amount,
        }],
    )
    .unwrap();

    // Assert state after swap (Query pool contract and user balance)
    let balance_user2_after = app
        .wrap()
        .query_balance(user2.clone(), TOKEN_B)
        .unwrap()
        .amount;
    assert!(balance_user2_after > balance_user2_before);
    let pool_balance_a_swap = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let pool_balance_b_swap = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_B)
        .unwrap()
        .amount;
    assert_eq!(pool_balance_a_swap, pool_balance_a_after + offer_amount);
    let output_b = balance_user2_after - balance_user2_before;
    assert_eq!(pool_balance_b_swap, pool_balance_b_after - output_b);

    // --- Withdraw liquidity by user1 (Send LP tokens to the pool contract) ---
    let user1_lp_balance = lp_balance.balance;
    let withdraw_hook = Cw20HookMsg::WithdrawLiquidity {};
    let user1_tokena_before_withdraw = app
        .wrap()
        .query_balance(user1.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let user1_tokenb_before_withdraw = app
        .wrap()
        .query_balance(user1.clone(), TOKEN_B)
        .unwrap()
        .amount;
    app.execute_contract(
        user1.clone(),
        lp_token_addr.clone(),
        &cw20::Cw20ExecuteMsg::Send {
            contract: pool_addr.to_string(), // Send hook to the pool contract
            amount: user1_lp_balance,
            msg: to_json_binary(&withdraw_hook).unwrap(),
        },
        &[],
    )
    .unwrap();

    // Assert state after withdraw (Query pool and user balances)
    let user1_final_a = app
        .wrap()
        .query_balance(user1.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let user1_final_b = app
        .wrap()
        .query_balance(user1.clone(), TOKEN_B)
        .unwrap()
        .amount;
    assert!(user1_final_a > user1_tokena_before_withdraw);
    assert!(user1_final_b > user1_tokenb_before_withdraw);
    let total_supply_final: TokenInfoResponse = app
        .wrap()
        .query_wasm_smart(lp_token_addr.clone(), &cw20::Cw20QueryMsg::TokenInfo {})
        .unwrap();
    assert_eq!(
        total_supply_final.total_supply + user1_lp_balance,
        total_supply_after.total_supply
    );
    let pool_balance_a_final = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_A)
        .unwrap()
        .amount;
    let pool_balance_b_final = app
        .wrap()
        .query_balance(pool_addr.clone(), TOKEN_B)
        .unwrap()
        .amount;
    let expected_pool_a = pool_balance_a_swap - (user1_final_a - user1_tokena_before_withdraw);
    let expected_pool_b = pool_balance_b_swap - (user1_final_b - user1_tokenb_before_withdraw);
    assert!(
        close_enough(pool_balance_a_final, expected_pool_a),
        "Final pool A balance mismatch"
    );
    assert!(
        close_enough(pool_balance_b_final, expected_pool_b),
        "Final pool B balance mismatch"
    );
}

/// Compare Uint128 values with +/- 1 tolerance
fn close_enough(a: Uint128, b: Uint128) -> bool {
    if a == b {
        return true;
    }
    let diff = if a > b { a - b } else { b - a };
    diff < Uint128::from(2u128)
}

#[test]
fn test_create_pool_errors() {
    // Use setup helper
    let (mut app, factory_addr, _factory_code_id, pool_code_id, _owner, user1, _user2) =
        setup_app();
    // Create the first pool successfully
    create_basic_pool(&mut app, &factory_addr, pool_code_id, &user1);

    // --- Test Pool Already Exists ---
    // Attempt to create the same pool again
    let create_msg = FactoryMsg::ExecuteMsg::CreatePool {
        pool_logic_code_id: pool_code_id,
        denom_a: TOKEN_A.to_string(),
        denom_b: TOKEN_B.to_string(),
    };
    let err = app
        .execute_contract(user1.clone(), factory_addr.clone(), &create_msg, &[])
        .unwrap_err();
    assert!(err.root_cause().to_string().contains("Pool already exists"));

    // --- Test Identical Denoms (Should fail in Factory CreatePool) ---
    let create_msg_same_denom = FactoryMsg::ExecuteMsg::CreatePool {
        pool_logic_code_id: pool_code_id,
        denom_a: TOKEN_A.to_string(),
        denom_b: TOKEN_A.to_string(),
    };
    let err_same = app
        .execute_contract(
            user1.clone(),
            factory_addr.clone(),
            &create_msg_same_denom,
            &[],
        )
        .unwrap_err();
    assert_eq!(
        err_same.root_cause().to_string(),
        "Denom A and Denom B must be different"
    );

    // --- Test Sending Funds on CreatePool (Factory should reject) ---
    let create_msg_funds = FactoryMsg::ExecuteMsg::CreatePool {
        pool_logic_code_id: pool_code_id,
        denom_a: "tokenC".to_string(),
        denom_b: "tokenD".to_string(),
    };
    app.sudo(cw_multi_test::SudoMsg::Bank(BankSudo::Mint {
        to_address: user1.to_string(),
        amount: vec![cosmwasm_std::coin(1000u128, "native_token")],
    }))
    .unwrap();
    let err_funds = app
        .execute_contract(
            user1.clone(),
            factory_addr.clone(),
            &create_msg_funds,
            &[cosmwasm_std::coin(1000u128, "native_token")],
        )
        .unwrap_err();
    assert!(err_funds
        .root_cause()
        .to_string()
        .contains("Cannot send funds when calling CreatePool"));
}

#[test]
fn test_add_liquidity_errors() {
    let (mut app, factory_addr, _factory_code_id, pool_code_id, _owner, user1, _user2) =
        setup_app();
    let (pool_addr, _lp_token_addr) =
        create_basic_pool(&mut app, &factory_addr, pool_code_id, &user1);

    // --- Test Add Zero Amount ---
    let add_msg_zero = PoolMsg::ExecuteMsg::AddLiquidity {};
    let err_zero_a = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &add_msg_zero,
            &[
                cosmwasm_std::coin(0u128, TOKEN_A),
                cosmwasm_std::coin(100u128, TOKEN_B),
            ], // Send 0 A
        )
        .unwrap_err();
    assert!(err_zero_a
        .root_cause()
        .to_string()
        .contains("Must provide both tokens"));

    let err_zero_b = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &add_msg_zero,
            &[
                cosmwasm_std::coin(100u128, TOKEN_A),
                cosmwasm_std::coin(0u128, TOKEN_B),
            ], // Send 0 B
        )
        .unwrap_err();
    assert!(err_zero_b
        .root_cause()
        .to_string()
        .contains("Must provide both tokens"));

    // --- Test Add Only One Token ---
    let add_msg_one = PoolMsg::ExecuteMsg::AddLiquidity {};
    let err_one = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &add_msg_one,
            &[cosmwasm_std::coin(100u128, TOKEN_A)], // Only send A
        )
        .unwrap_err();
    assert!(err_one
        .root_cause()
        .to_string()
        .contains("Must provide both tokens"));

    // --- Test Ratio Mismatch ---
    let add_msg_slippage = PoolMsg::ExecuteMsg::AddLiquidity {};
    app.execute_contract(
        user1.clone(),
        pool_addr.clone(),
        &add_msg_slippage,
        &[
            cosmwasm_std::coin(1000u128, TOKEN_A),
            cosmwasm_std::coin(1000u128, TOKEN_B),
        ],
    )
    .unwrap();
}

#[test]
fn test_swap_errors() {
    let (mut app, factory_addr, _factory_code_id, pool_code_id, _owner, user1, _user2) =
        setup_app();
    let (pool_addr, _lp_token_addr) =
        create_basic_pool(&mut app, &factory_addr, pool_code_id, &user1);

    // --- Test Swap with Non-Pool Token ---
    let swap_msg_wrong_offer = PoolMsg::ExecuteMsg::Swap {
        offer_denom: "tokenC".into(),
        min_receive: Uint128::one(),
    };
    let err_wrong_offer = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &swap_msg_wrong_offer,
            &[cosmwasm_std::coin(100u128, "tokenC")],
        )
        .unwrap_err();
    assert!(err_wrong_offer
        .root_cause()
        .to_string()
        .contains("Cannot Sub with given operands"));

    // --- Test Swap Zero Offer ---
    let swap_msg_zero = PoolMsg::ExecuteMsg::Swap {
        offer_denom: TOKEN_A.into(),
        min_receive: Uint128::one(),
    };
    let err_zero = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &swap_msg_zero,
            &[cosmwasm_std::coin(0u128, TOKEN_A)],
        )
        .unwrap_err();
    assert!(err_zero
        .root_cause()
        .to_string()
        .contains("Cannot transfer empty coins amount"));

    // --- Test Swap Wrong Denom Sent (Funds mismatch message) ---
    let swap_msg_wrong_denom = PoolMsg::ExecuteMsg::Swap {
        offer_denom: TOKEN_A.into(),
        min_receive: Uint128::one(),
    };
    let err_wrong_denom = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &swap_msg_wrong_denom,
            &[cosmwasm_std::coin(100u128, TOKEN_B)], // Sending TOKEN_B but specifying TOKEN_A in msg
        )
        .unwrap_err();
    assert!(err_wrong_denom
        .root_cause()
        .to_string()
        .contains("No matching offer coin found"));

    // --- Test Swap Insufficient Minimum Receive ---
    let swap_msg_min_recv = PoolMsg::ExecuteMsg::Swap {
        offer_denom: TOKEN_A.into(),
        min_receive: Uint128::new(200000),
    };
    let err_min_recv = app
        .execute_contract(
            user1.clone(),
            pool_addr.clone(),
            &swap_msg_min_recv,
            &[cosmwasm_std::coin(1000u128, TOKEN_A)],
        )
        .unwrap_err();
    assert!(err_min_recv
        .root_cause()
        .to_string()
        .contains("Output amount "));
    assert!(err_min_recv
        .root_cause()
        .to_string()
        .contains(" less than minimum requested "));
}

#[test]
fn test_withdraw_errors() {
    let (mut app, factory_addr, _factory_code_id, pool_code_id, owner, user1, _user2) = setup_app();
    let (pool_addr, lp_token_addr) =
        create_basic_pool(&mut app, &factory_addr, pool_code_id, &user1);

    // --- Test Withdraw From Non-LP Token (Pool hook should reject sender) ---
    let dummy_code_id = app.store_code(cw20_contract());
    let dummy_lp = app
        .instantiate_contract(
            dummy_code_id,
            owner.clone(),
            &cw20_base::msg::InstantiateMsg {
                name: "Dummy".into(),
                symbol: "DUM".into(),
                decimals: 6,
                initial_balances: vec![],
                mint: None,
                marketing: None,
            },
            &[],                   // funds
            "DummyLP".to_string(), // label
            None,                  // admin
        )
        .unwrap();

    let withdraw_hook = Cw20HookMsg::WithdrawLiquidity {};
    let send_msg_wrong_lp = cw20::Cw20ExecuteMsg::Send {
        contract: pool_addr.to_string(),
        amount: Uint128::new(100),
        msg: to_json_binary(&withdraw_hook).unwrap(),
    };
    let err_wrong_lp = app
        .execute_contract(owner.clone(), dummy_lp.clone(), &send_msg_wrong_lp, &[])
        .unwrap_err();
    assert!(err_wrong_lp
        .root_cause()
        .to_string()
        .contains("Cannot Sub with given operands"));

    // --- Test Withdraw Zero Amount (Pool hook should reject) ---
    let withdraw_hook_zero = Cw20HookMsg::WithdrawLiquidity {};
    let send_msg_zero = cw20::Cw20ExecuteMsg::Send {
        contract: pool_addr.to_string(),
        amount: Uint128::zero(),
        msg: to_json_binary(&withdraw_hook_zero).unwrap(),
    };
    let err_zero_withdraw = app
        .execute_contract(user1.clone(), lp_token_addr.clone(), &send_msg_zero, &[])
        .unwrap_err();
    assert!(err_zero_withdraw
        .root_cause()
        .to_string()
        .contains("Withdraw amount cannot be zero"));
}
