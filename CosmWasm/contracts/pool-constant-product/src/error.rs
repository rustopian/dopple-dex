use cosmwasm_std::{Addr, DivideByZeroError, OverflowError, StdError, Uint128};
use cw_utils::ParseReplyError;
use thiserror::Error;

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    DivideByZeroError(#[from] DivideByZeroError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    ConversionOverflowError(#[from] cosmwasm_std::ConversionOverflowError),

    #[error("Unauthorized (expected factory: {expected}) - Only factory can initialize LP token")]
    UnauthorizedFactory { expected: Addr },

    #[error(
        "Unauthorized (expected LP token: {expected}) - Only own LP token can trigger withdraw"
    )]
    UnauthorizedLpToken { expected: Addr },

    #[error("Invalid CW20 hook message")]
    InvalidCw20HookMsg {},

    #[error("Withdraw amount cannot be zero")]
    ZeroWithdrawAmount {},

    #[error("Initial liquidity amounts must be positive")]
    ZeroInitialLiquidity {},

    #[error("Initial liquidity too low to mint LP tokens")]
    InitialLiquidityTooLow {},

    #[error("Cannot calculate shares with zero total supply (should use initial calculation)")]
    CalculateSharesWithZeroSupply {},

    #[error("Cannot calculate shares against zero reserves for existing pool")]
    CalculateSharesWithZeroReserve {},

    #[error("Cannot validate ratio against zero reserves for existing pool")]
    ValidateRatioWithZeroReserve {},

    #[error("Deposit ratio mismatch exceeds slippage tolerance")]
    DepositRatioMismatch {},

    #[error("Cannot swap against empty reserves")]
    SwapAgainstEmptyReserve {},

    #[error("Invalid denom received: {denom}")]
    InvalidLiquidityDenom { denom: String },

    #[error("Must provide both tokens to add liquidity")]
    MissingLiquidityToken {},

    #[error("No matching offer coin found for denom {denom}")]
    NoMatchingOfferCoin { denom: String },

    #[error("Offer amount must be positive")]
    ZeroOfferAmount {},

    #[error("Output amount {output} less than minimum requested {min_receive}")]
    SwapMinimumReceiveViolation {
        output: Uint128,
        min_receive: Uint128,
    },

    #[error("Pool is not initialized with LP token address yet")]
    NotInitialized {},

    #[error("Unknown reply id: {id}")]
    UnknownReplyId { id: u64 },

    #[error("Missing reply data")]
    MissingReplyData {},

    #[error("Error parsing instantiate reply: {0}")]
    ParseInstantiateReplyError(#[from] ParseReplyError),

    #[error("Bank query failed for denom {denom}: {error}")]
    BankQueryFailed { denom: String, error: StdError },

    #[error("CW20 token query failed for contract {contract}: {error}")]
    TokenQueryFailed { contract: Addr, error: StdError },
}
