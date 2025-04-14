use crate::error::PoolError;
use solana_program::{
    account_info::AccountInfo, bpf_loader, bpf_loader_upgradeable, msg,
    program_error::ProgramError, program_option::COption, program_pack::Pack, pubkey::Pubkey,
    sysvar::rent::Rent,
};
use spl_associated_token_account::get_associated_token_address;
use spl_token::{
    state::{Account as TokenAccount, AccountState, Mint},
    ID as TOKEN_PROGRAM_ID,
};

/// Struct to hold PDA information
pub struct PdaInfo {
    /// The derived program derived address
    pub address: Pubkey,
    /// The bump seed used in the PDA derivation
    pub bump: u8,
}

/// Get the pool PDA and bump seed
pub fn find_pool_address(
    program_id: &Pubkey,
    mint_a: &Pubkey,
    mint_b: &Pubkey,
    plugin_program_id: &Pubkey,
    plugin_state_pubkey: &Pubkey,
) -> (Pubkey, u8) {
    let (sorted_mint_a, sorted_mint_b) = if mint_a < mint_b {
        (mint_a, mint_b)
    } else {
        (mint_b, mint_a)
    };

    Pubkey::find_program_address(
        &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_program_id.as_ref(),
            plugin_state_pubkey.as_ref(),
        ],
        program_id,
    )
}

/// Get the pool seeds with bump for signing
pub fn get_pool_seeds<'a>(
    mint_a: &'a Pubkey,
    mint_b: &'a Pubkey,
    plugin_program_id: &'a Pubkey,
    plugin_state_pubkey: &'a Pubkey,
    bump_seed: &'a [u8],
) -> [&'a [u8]; 6] {
    let (sorted_mint_a, sorted_mint_b) = if mint_a < mint_b {
        (mint_a, mint_b)
    } else {
        (mint_b, mint_a)
    };

    [
        b"pool",
        sorted_mint_a.as_ref(),
        sorted_mint_b.as_ref(),
        plugin_program_id.as_ref(),
        plugin_state_pubkey.as_ref(),
        bump_seed,
    ]
}

/// Checks if an account is rent-exempt.
/// Kept for use in InitializePool, but generally not called elsewhere.
pub fn validate_rent_exemption(
    account_info: &AccountInfo,
    rent: &Rent,
) -> Result<(), ProgramError> {
    if !rent.is_exempt(account_info.lamports(), account_info.data_len()) {
        msg!(
            "Account {} with lamports {} and data len {} is not rent exempt",
            account_info.key,
            account_info.lamports(),
            account_info.data_len()
        );
        Err(PoolError::AccountNotRentExempt.into())
    } else {
        Ok(())
    }
}

/// Validates a token account intended as a pool vault.
/// Checks: ATA derivation, Token Program owner, Initialized, Internal Owner (Pool PDA), Mint.
pub fn validate_pool_vault(
    vault_info: &AccountInfo,
    expected_owner_pda: &Pubkey,
    expected_mint: &Pubkey,
) -> Result<(), ProgramError> {
    // --- Check 1: Is the vault account key the correct derived ATA? ---
    let expected_vault_ata = get_associated_token_address(expected_owner_pda, expected_mint);
    if vault_info.key != &expected_vault_ata {
        msg!(
            "Vault ATA Error: Expected {}, got {}",
            expected_vault_ata,
            vault_info.key
        );
        return Err(PoolError::IncorrectVaultATA.into());
    }

    // --- Check 2: Ownership by Token Program ---
    if vault_info.owner != &TOKEN_PROGRAM_ID {
        msg!(
            "Vault Error: Account {} owned by {}, expected {}",
            vault_info.key,
            vault_info.owner,
            TOKEN_PROGRAM_ID
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    // --- Check 3: Unpack and Check Initialized State ---
    let token_account_data = TokenAccount::unpack(&vault_info.data.borrow())
        .map_err(|_| PoolError::UnpackAccountFailed)?;

    if token_account_data.state != AccountState::Initialized {
        msg!("Vault Error: Account {} is not initialized", vault_info.key);
        return Err(PoolError::InvalidAccountData.into());
    }

    // --- Check 4: Internal Owner matches Pool PDA ---
    if &token_account_data.owner != expected_owner_pda {
        msg!(
            "Vault Error: Account {} owner {} does not match expected PDA {}",
            vault_info.key,
            token_account_data.owner,
            expected_owner_pda
        );
        return Err(PoolError::InvalidVaultOwner.into());
    }

    // --- Check 5: Mint matches expected mint ---
    if &token_account_data.mint != expected_mint {
        msg!(
            "Vault Error: Account {} mint {} does not match expected mint {}",
            vault_info.key,
            token_account_data.mint,
            expected_mint
        );
        return Err(PoolError::TokenMintMismatch.into());
    }

    Ok(())
}

/// Validates basic properties of any SPL Token account.
/// Checks: Token Program owner, Initialized, Internal Owner, Mint.
pub fn validate_token_account_basic(
    account_info: &AccountInfo,
    expected_owner: &Pubkey,
    expected_mint: &Pubkey,
) -> Result<TokenAccount, ProgramError> {
    // Check ownership by Token Program
    if account_info.owner != &TOKEN_PROGRAM_ID {
        msg!(
            "Token Account Error: Account {} owned by {}, expected {}",
            account_info.key,
            account_info.owner,
            TOKEN_PROGRAM_ID
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    // Unpack token account data
    let token_account_data = TokenAccount::unpack(&account_info.data.borrow())
        .map_err(|_| PoolError::UnpackAccountFailed)?;

    // Check if initialized (state check)
    if token_account_data.state != AccountState::Initialized {
        msg!(
            "Token Account Error: Account {} is not initialized",
            account_info.key
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    // Check owner field inside the token account data
    if &token_account_data.owner != expected_owner {
        msg!(
            "Token Account Error: Account {} owner {} does not match expected owner {}",
            account_info.key,
            token_account_data.owner,
            expected_owner
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    // Check mint
    if &token_account_data.mint != expected_mint {
        msg!(
            "Token Account Error: Account {} mint {} does not match expected mint {}",
            account_info.key,
            token_account_data.mint,
            expected_mint
        );
        return Err(PoolError::TokenMintMismatch.into());
    }

    Ok(token_account_data)
}

/// Validates basic properties of an SPL Mint account.
/// Checks: Token Program owner, Initialized.
pub fn validate_mint_basic(
    mint_info: &AccountInfo,
) -> Result<Mint, ProgramError> {
    // Check ownership by Token Program
    if mint_info.owner != &TOKEN_PROGRAM_ID {
        msg!(
            "Mint Error: Account {} owned by {}, expected {}",
            mint_info.key,
            mint_info.owner,
            TOKEN_PROGRAM_ID
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    // Unpack Mint data
    let mint_data =
        Mint::unpack(&mint_info.data.borrow()).map_err(|_| PoolError::UnpackAccountFailed)?;

    // Check if initialized
    if !mint_data.is_initialized {
        msg!("Mint Error: Account {} is not initialized", mint_info.key);
        return Err(PoolError::InvalidAccountData.into());
    }

    Ok(mint_data)
}

/// Validates properties of an LP Mint account's data (authority, freeze authority).
/// Assumes basic mint validation (owner, init) has already passed.
pub fn validate_lp_mint_properties(
    mint_data: &Mint,
    expected_authority: &Pubkey,
) -> Result<(), ProgramError> {
    // Check mint authority
    if mint_data.mint_authority != COption::Some(*expected_authority) {
        msg!(
            "LP Mint Error: Incorrect authority {:?}, expected {}",
            mint_data.mint_authority,
            expected_authority
        );
        return Err(PoolError::InvalidMintAuthority.into());
    }

    // Check freeze authority is None
    if mint_data.freeze_authority.is_some() {
        msg!(
            "LP Mint Error: Freeze authority set {:?}",
            mint_data.freeze_authority
        );
        return Err(PoolError::FreezeAuthoritySet.into());
    }
    Ok(())
}

/// Validates that an LP Mint account's data shows zero supply.
/// Assumes basic mint validation has passed.
pub fn validate_lp_mint_zero_supply(mint_data: &Mint) -> Result<(), ProgramError> {
    if mint_data.supply != 0 {
        msg!(
            "LP Mint Error: Non-zero initial supply {}",
            mint_data.supply
        );
        return Err(PoolError::NonZeroLpSupply.into());
    }
    Ok(())
}

/// Validates that the provided account's key matches the expected program ID.
pub fn validate_program_id(
    account_info: &AccountInfo,
    expected_program_id: &Pubkey,
) -> Result<(), ProgramError> {
    if account_info.key != expected_program_id {
        msg!(
            "Program ID Error: Expected {}, got {}",
            expected_program_id,
            account_info.key
        );
        Err(PoolError::IncorrectProgramId.into())
    } else {
        Ok(())
    }
}

/// Validates that the provided account is executable and owned by a BPF loader.
pub fn validate_executable(account_info: &AccountInfo) -> Result<(), ProgramError> {
    if !account_info.executable {
        msg!("Exec Error: Account {} is not executable", account_info.key);
        return Err(PoolError::AccountNotExecutable.into());
    }

    // Check owner is a BPF loader
    if account_info.owner != &bpf_loader::id()
        && account_info.owner != &bpf_loader_upgradeable::id()
    {
        msg!(
            "Exec Error: Account {} owned by {}, expected a BPF loader",
            account_info.key,
            account_info.owner
        );
        return Err(PoolError::InvalidAccountData.into());
    }

    Ok(())
}
