use solana_program::program_error::ProgramError;
use thiserror::Error;

/// Custom errors that can be returned by the Pool program.
#[derive(Error, Debug, Copy, Clone, PartialEq)]
pub enum PoolError {
    /// Invalid instruction data passed.
    #[error("Invalid instruction data")]
    InvalidInstructionData,

    /// Missing required signature.
    #[error("Missing required signature")]
    MissingRequiredSignature,

    /// An argument provided was invalid.
    #[error("Invalid argument")]
    InvalidArgument,

    /// An account's data was invalid.
    #[error("Invalid account data")]
    InvalidAccountData,

    /// Not enough funds to perform the operation.
    #[error("Insufficient funds")]
    InsufficientFunds,

    /// An arithmetic operation overflowed.
    #[error("Arithmetic overflow")]
    ArithmeticOverflow,

    /// Failed to unpack an account.
    #[error("Failed to unpack account")]
    UnpackAccountFailed,

    /// Failed to pack state into account data.
    #[error("Failed to pack state")]
    PackStateFailed,

    /// Failed CPI call.
    #[error("CPI Error")]
    CPIError,

    /// Zero amount provided for an operation.
    #[error("Zero amount")]
    ZeroAmount,

    /// Calculated amount is less than the minimum required.
    #[error("Slippage limit exceeded")]
    SlippageLimitExceeded,

    /// Pool state owner is invalid
    #[error("Invalid pool state owner")]
    InvalidPoolStateOwner,

    /// Vault account owner is invalid
    #[error("Invalid vault owner")]
    InvalidVaultOwner,

    /// LP Mint account owner is invalid
    #[error("Invalid LP Mint owner")]
    InvalidLpMintOwner,

    /// Token mint mismatch
    #[error("Token mint mismatch")]
    TokenMintMismatch,

    /// Vault account mismatch
    #[error("Vault account mismatch")]
    VaultMismatch,

    /// LP mint account mismatch
    #[error("LP mint mismatch")]
    LpMintMismatch,

    /// Plugin program ID mismatch
    #[error("Plugin program ID mismatch")]
    PluginProgramIdMismatch,

    /// Plugin state pubkey mismatch
    #[error("Plugin state pubkey mismatch")]
    PluginStatePubkeyMismatch,

    /// Plugin compute failed
    #[error("Plugin computation failed")]
    PluginComputeFailed,

    /// Expected PDA doesn't match provided account
    #[error("Incorrect pool PDA provided")]
    IncorrectPoolPDA,

    /// LP Supply is zero
    #[error("LP Supply is zero")]
    ZeroLpSupply,

    /// Account is not rent exempt
    #[error("Account not rent exempt")]
    AccountNotRentExempt,

    /// Invalid mint authority
    #[error("Invalid mint authority")]
    InvalidMintAuthority,

    /// LP Mint supply was not zero on init
    #[error("LP Mint initial supply must be zero")]
    NonZeroLpSupply,

    /// LP Mint freeze authority is set
    #[error("LP Mint freeze authority must not be set")]
    FreezeAuthoritySet,

    /// Provided program ID is incorrect
    #[error("Incorrect program ID provided")]
    IncorrectProgramId,

    /// Provided account is not executable
    #[error("Account not executable")]
    AccountNotExecutable,

    /// Pool mints must be different
    #[error("Pool mints must be different")]
    MintsMustBeDifferent,

    /// Provided vault account is not the correct ATA
    #[error("Incorrect vault ATA provided")]
    IncorrectVaultATA,

    /// Invalid mint account provided (e.g., native SOL used for LP mint)
    #[error("Invalid mint account")]
    InvalidMint,
}

impl From<PoolError> for ProgramError {
    fn from(e: PoolError) -> Self {
        // Log the error source for easier debugging
        // msg!("Error: {}", e);
        ProgramError::Custom(e as u32)
    }
}
