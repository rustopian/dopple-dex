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

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Unknown reply id: {id}")]
    UnknownReplyId { id: u64 },

    #[error("Missing reply data")]
    MissingReplyData {},

    #[error("Error parsing reply data: {error}")]
    ParseReplyError { error: String },

    #[error("Error parsing instantiate reply: {0}")]
    ParseInstantiateReplyError(#[from] ParseReplyError),

    #[error("Creator address not found in pending pool")]
    PendingPoolCreatorMissing {},

    #[error("Cannot send funds when calling CreatePool. Provide initial liquidity via ProvideInitialLiquidity.")]
    FundsSentOnCreatePool {},

    #[error("Another pool creation is already pending. Please wait.")]
    PoolCreationPending {},

    #[error("Admin cannot be set to None.")]
    AdminCannotBeNone {},

    #[error("Cannot provide initial liquidity, pool already contains liquidity.")]
    PoolAlreadyHasLiquidity {},

    #[error("Denom A and Denom B must be different")]
    IdenticalDenoms {},

    #[error("Pool already exists for denoms {denom1} and {denom2}")]
    PoolAlreadyExists { denom1: String, denom2: String },

    #[error("Pool not found for denoms {denom1} and {denom2}")]
    PoolNotFound { denom1: String, denom2: String },

    #[error("Do not send funds with CreatePool unless initial_amounts are provided")]
    FundsSentWithoutInitialAmounts {},

    #[error("WithdrawLiquidity must be triggered by sending LP tokens to the contract")]
    WithdrawRequiresCw20Receive {},

    #[error("Initial liquidity must be > 0 for both tokens")]
    InitialLiquidityZero {},

    #[error("Must send two types of tokens for initial liquidity")]
    InvalidInitialFundsLength {},

    #[error("Attached amount for {denom} ({sent}) does not match expected ({expected})")]
    InitialFundMismatch {
        denom: String,
        sent: Uint128,
        expected: Uint128,
    },

    #[error("Unexpected fund denom: {denom}")]
    UnexpectedInitialDenom { denom: String },

    #[error("Did not send required initial liquidity funds")]
    MissingInitialFunds {},

    #[error("Invalid denom received: {denom}")]
    InvalidLiquidityDenom { denom: String },

    #[error("Must provide both tokens to add liquidity")]
    MissingLiquidityToken {},

    #[error("No matching offer coin found for denom {denom}")]
    NoMatchingOfferCoin { denom: String },

    #[error("Offer amount must be positive")]
    ZeroOfferAmount {},

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

    #[error("Bank query failed for denom {denom}: {error}")]
    BankQueryFailed { denom: String, error: StdError },

    #[error("CW20 token query failed for contract {contract}: {error}")]
    TokenQueryFailed { contract: Addr, error: StdError },

    #[error("Output amount {output} less than minimum requested {min_receive}")]
    SwapMinimumReceiveViolation {
        output: Uint128,
        min_receive: Uint128,
    },

    #[error("Withdraw amount cannot be zero")]
    ZeroWithdrawAmount {},
}
