use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program::{invoke, invoke_signed},
    program_pack::Pack,
    pubkey::Pubkey,
    system_instruction,
    sysvar::{rent::Rent, Sysvar},
};
use spl_token::state::Account as TokenAccount;

use crate::error::PoolError;
use crate::instruction::PoolInstruction;
use crate::pda::{
    find_pool_address, validate_executable, validate_lp_mint_properties,
    validate_lp_mint_zero_supply, validate_mint_basic, validate_pool_vault, validate_program_id,
    validate_rent_exemption, validate_token_account_basic,
};
use crate::state::PoolState;

/// For plugin <-> pool communication
/// We'll reuse a struct for reading plugin's computed results.
#[derive(BorshDeserialize, BorshSerialize, Debug, Default)]
pub struct PluginCalcResult {
    /// Actual amount of token A deposited/withdrawn (relevant for Add/Remove Liquidity)
    pub actual_a: u64,
    /// Actual amount of token B deposited/withdrawn (relevant for Add/Remove Liquidity)
    pub actual_b: u64,
    /// Number of LP shares minted (relevant for Add Liquidity)
    pub shares_to_mint: u64,
    /// Amount of token A withdrawn (relevant for Remove Liquidity)
    pub withdraw_a: u64,
    /// Amount of token B withdrawn (relevant for Remove Liquidity)
    pub withdraw_b: u64,
    /// Amount of output token calculated (relevant for Swap)
    pub amount_out: u64,
}

/// Processes instructions for the Pool program.
pub struct Processor;
impl Processor {
    /// Main processing function dispatching to specific instruction handlers.
    pub fn process(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        instr_data: &[u8],
    ) -> ProgramResult {
        let instruction = PoolInstruction::try_from_slice(instr_data)
            .map_err(|_| PoolError::InvalidInstructionData)?;

        match instruction {
            PoolInstruction::InitializePool => Self::process_initialize_pool(program_id, accounts),
            PoolInstruction::AddLiquidity { amount_a, amount_b } => {
                Self::process_add_liquidity(program_id, accounts, amount_a, amount_b)
            }
            PoolInstruction::RemoveLiquidity { amount_lp } => {
                Self::process_remove_liquidity(program_id, accounts, amount_lp)
            }
            PoolInstruction::Swap { amount_in, min_out } => {
                Self::process_swap(program_id, accounts, amount_in, min_out)
            }
        }
    }

    fn process_initialize_pool(program_id: &Pubkey, accounts: &[AccountInfo]) -> ProgramResult {
        msg!("Pool: process_initialize_pool entry");
        let acc_iter = &mut accounts.iter();
        let payer_acc = next_account_info(acc_iter)?; // 0
        let pool_state_acc = next_account_info(acc_iter)?; // 1
        let vault_a_acc = next_account_info(acc_iter)?; // 2
        let vault_b_acc = next_account_info(acc_iter)?; // 3
        let lp_mint_acc = next_account_info(acc_iter)?; // 4
        let mint_a_acc = next_account_info(acc_iter)?; // 5
        let mint_b_acc = next_account_info(acc_iter)?; // 6
        let plugin_prog_acc = next_account_info(acc_iter)?; // 7
        let plugin_state_acc = next_account_info(acc_iter)?; // 8
        let system_acc = next_account_info(acc_iter)?; // 9
        let rent_acc = next_account_info(acc_iter)?; // 10
        let token_prog_acc = next_account_info(acc_iter)?; // 11

        // --- Initial Validations ---
        msg!("Pool Init: Validating accounts...");
        // 0. Payer must sign
        if !payer_acc.is_signer {
            msg!("Payer did not sign");
            return Err(PoolError::MissingRequiredSignature.into());
        }

        // 9. System Program ID
        validate_program_id(system_acc, &solana_program::system_program::id())?;

        // 10. Rent Sysvar ID
        validate_program_id(rent_acc, &solana_program::sysvar::rent::id())?;
        let rent = Rent::from_account_info(rent_acc)?;

        // 11. Token Program ID
        validate_program_id(token_prog_acc, &spl_token::id())?;

        // 7. Plugin Program Account (Executable? Owned by Loader?)
        validate_executable(plugin_prog_acc)?;

        // 8. Plugin State Account (Rent-exempt?)
        validate_rent_exemption(plugin_state_acc, &rent)?;

        // 5 & 6: Mint A & B must be different
        if mint_a_acc.key == mint_b_acc.key {
            msg!("Mint A and Mint B cannot be the same");
            return Err(PoolError::MintsMustBeDifferent.into());
        }

        // --- PDA Derivation & Validation ---
        msg!("Pool Init: Deriving pool PDA...");
        // Sort the mint addresses
        let (sort_mint_a, sort_mint_b) = if mint_a_acc.key < mint_b_acc.key {
            (mint_a_acc.key, mint_b_acc.key)
        } else {
            (mint_b_acc.key, mint_a_acc.key)
        };

        // Derive the pool PDA
        let seeds = &[
            b"pool",
            sort_mint_a.as_ref(),
            sort_mint_b.as_ref(),
            plugin_prog_acc.key.as_ref(),
            plugin_state_acc.key.as_ref(),
        ];
        let (expected_pool_pda, bump) = Pubkey::find_program_address(seeds, program_id);
        if &expected_pool_pda != pool_state_acc.key {
            msg!(
                "Pool ERROR: Expected pool pda {}, got {}",
                expected_pool_pda,
                pool_state_acc.key
            );
            return Err(PoolError::IncorrectPoolPDA.into());
        }

        // --- Mint & Vault Validations (using PDA and Rent) ---
        msg!("Pool Init: Validating Mints and Vaults...");
        // 5. Mint A (Basic Mint Checks + Rent)
        let _mint_a_data = validate_mint_basic(mint_a_acc)?;
        validate_rent_exemption(mint_a_acc, &rent)?;

        // 6. Mint B (Basic Mint Checks + Rent)
        let _mint_b_data = validate_mint_basic(mint_b_acc)?;
        validate_rent_exemption(mint_b_acc, &rent)?;

        // 4. LP Mint (Specific LP Mint Checks + Rent)
        let lp_mint_data = validate_mint_basic(lp_mint_acc)?;
        validate_lp_mint_properties(&lp_mint_data, &expected_pool_pda)?;
        validate_lp_mint_zero_supply(&lp_mint_data)?;
        validate_rent_exemption(lp_mint_acc, &rent)?;

        // 2. Vault A
        validate_pool_vault(vault_a_acc, &expected_pool_pda, mint_a_acc.key)?;
        // Rent implicitly checked by ATA creation on client, not checked here

        // 3. Vault B
        validate_pool_vault(vault_b_acc, &expected_pool_pda, mint_b_acc.key)?;
        // Rent implicitly checked by ATA creation on client, not checked here
        msg!("Pool Init: All account validations passed.");

        // --- Account Creation & State Initialization ---
        msg!("Pool Init: Constructing initial state...");

        // Construct initial PoolState data to get its serialized size
        let initial_pool_data = PoolState {
            token_mint_a: *mint_a_acc.key,
            token_mint_b: *mint_b_acc.key,
            vault_a: *vault_a_acc.key,
            vault_b: *vault_b_acc.key,
            lp_mint: *lp_mint_acc.key,
            total_lp_supply: 0,
            bump,
            plugin_program_id: *plugin_prog_acc.key,
            plugin_state_pubkey: *plugin_state_acc.key,
        };
        let pool_data_bytes = initial_pool_data.try_to_vec()?;
        let pool_space = pool_data_bytes.len(); // Use serialized length

        // Restore account creation CPI using serialized size
        msg!("Pool: Preparing invoke_signed create_account...");
        // let pool_space = std::mem::size_of::<PoolState>(); // Old way
        let needed_lamports = rent.minimum_balance(pool_space);
        msg!(
            "  Space (serialized): {}, Lamports: {}",
            pool_space,
            needed_lamports
        );
        invoke_signed(
            &system_instruction::create_account(
                payer_acc.key,
                pool_state_acc.key,
                needed_lamports,
                pool_space as u64, // Use serialized size
                program_id,        // Owner is self
            ),
            // Accounts for the CPI call itself
            &[
                payer_acc.clone(),
                pool_state_acc.clone(), // The account being created
                system_acc.clone(),
            ],
            // Seeds for signing as the PDA
            &[&[
                b"pool",
                sort_mint_a.as_ref(),
                sort_mint_b.as_ref(),
                plugin_prog_acc.key.as_ref(),
                plugin_state_acc.key.as_ref(),
                &[bump],
            ]],
        )?;
        msg!("Pool: invoke_signed successful.");

        // Write the already serialized pool state bytes
        msg!("Pool: Writing serialized PoolState...");
        // pool_data.serialize(&mut *pool_state_acc.data.borrow_mut())?; // Old way
        let mut account_data_borrow = pool_state_acc.data.borrow_mut();
        account_data_borrow.copy_from_slice(&pool_data_bytes);
        // Or alternatively, if you prefer deserializing/reserializing:
        // let mut pool_data = PoolState::try_from_slice(&account_data_borrow)?;
        // pool_data = initial_pool_data; // Assign values if needed, though they are the same here
        // pool_data.serialize(&mut *account_data_borrow)?;
        msg!("Pool: Initialized state written successfully.");

        Ok(())
    }

    fn process_add_liquidity(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        amount_a: u64,
        amount_b: u64,
    ) -> ProgramResult {
        msg!("Pool AddLiq: Processing");
        let acc_iter = &mut accounts.iter();
        let user_acc = next_account_info(acc_iter)?; // 0
        let pool_state_acc = next_account_info(acc_iter)?; // 1
        let vault_a_acc = next_account_info(acc_iter)?; // 2
        let vault_b_acc = next_account_info(acc_iter)?; // 3
        let lp_mint_acc = next_account_info(acc_iter)?; // 4
        let user_token_a_acc = next_account_info(acc_iter)?; // 5
        let user_token_b_acc = next_account_info(acc_iter)?; // 6
        let user_lp_acc = next_account_info(acc_iter)?; // 7
        let token_prog_acc = next_account_info(acc_iter)?; // 8
        let plugin_prog_acc = next_account_info(acc_iter)?; // 9
        let plugin_state_acc = next_account_info(acc_iter)?; // 10

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let mut pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;

        // --- PDA Re-derivation & Pool State Check ---
        let (expected_pda, _bump) = find_pool_address(
            program_id,
            &pool_data.token_mint_a,
            &pool_data.token_mint_b,
            &pool_data.plugin_program_id,
            &pool_data.plugin_state_pubkey,
        );
        if &expected_pda != pool_state_acc.key {
            return Err(PoolError::IncorrectPoolPDA.into());
        }

        // --- Account Key Checks vs Pool State ---
        if vault_a_acc.key != &pool_data.vault_a {
            return Err(PoolError::VaultMismatch.into());
        }
        if vault_b_acc.key != &pool_data.vault_b {
            return Err(PoolError::VaultMismatch.into());
        }
        if lp_mint_acc.key != &pool_data.lp_mint {
            return Err(PoolError::LpMintMismatch.into());
        }
        if plugin_prog_acc.key != &pool_data.plugin_program_id {
            return Err(PoolError::PluginProgramIdMismatch.into());
        }
        if plugin_state_acc.key != &pool_data.plugin_state_pubkey {
            return Err(PoolError::PluginStatePubkeyMismatch.into());
        }

        // --- Account Data Validations ---
        validate_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
        validate_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;
        // Validate LP Mint (Properties only, supply can be non-zero)
        let lp_mint_data = validate_mint_basic(lp_mint_acc)?;
        validate_lp_mint_properties(&lp_mint_data, &expected_pda)?;

        let _user_token_a_data =
            validate_token_account_basic(user_token_a_acc, user_acc.key, &pool_data.token_mint_a)?;
        let _user_token_b_data =
            validate_token_account_basic(user_token_b_acc, user_acc.key, &pool_data.token_mint_b)?;
        let _user_lp_data =
            validate_token_account_basic(user_lp_acc, user_acc.key, &pool_data.lp_mint)?;
        // Plugin accounts are implicitly checked by CPI

        // --- Get Reserves (safe after validation) ---
        let vault_a_data = TokenAccount::unpack(&vault_a_acc.data.borrow())?;
        let vault_b_data = TokenAccount::unpack(&vault_b_acc.data.borrow())?;
        let reserve_a = vault_a_data.amount;
        let reserve_b = vault_b_data.amount;

        // Log keys before CPI setup
        msg!("Pool->Plugin CPI Prep: Pool PDA: {}", pool_state_acc.key);
        msg!(
            "Pool->Plugin CPI Prep: Plugin Prog ID (from state): {}",
            pool_data.plugin_program_id
        );
        msg!(
            "Pool->Plugin CPI Prep: Plugin State Acc Key (from accounts): {}",
            plugin_state_acc.key
        );
        msg!(
            "Pool->Plugin CPI Prep: Plugin Prog Acc Key (from accounts): {}",
            plugin_prog_acc.key
        );

        // CPI to plugin -- Inlined
        let ix_data =
            constant_product_plugin::instruction::PluginInstruction::ComputeAddLiquidity {
                reserve_a,
                reserve_b,
                deposit_a: amount_a, // Use original amount_a
                deposit_b: amount_b, // Use original amount_b
                total_lp_supply: pool_data.total_lp_supply,
            }
            .try_to_vec()?;
        let ix = solana_program::instruction::Instruction {
            program_id: pool_data.plugin_program_id,
            accounts: vec![
                // Mark as writable (implicit via accounts list), NOT signer (false)
                solana_program::instruction::AccountMeta::new(*plugin_state_acc.key, false),
            ],
            data: ix_data,
        };
        msg!("Pool: About to invoke plugin for AddLiquidity");
        invoke(
            &ix,
            &[
                plugin_prog_acc.clone(),
                plugin_state_acc.clone(), // Writable passed here
            ],
        )?;
        msg!("Pool: Plugin invoke successful (returned Ok)");

        // Read the plugin result from plugin_state
        let plugin_calc = PluginCalcResult::deserialize(&mut &plugin_state_acc.data.borrow()[..])?;
        let actual_a = plugin_calc.actual_a;
        let actual_b = plugin_calc.actual_b;
        let shares_to_mint = plugin_calc.shares_to_mint;
        if shares_to_mint == 0 {
            return Err(PoolError::ZeroAmount.into());
        }

        // Transfer actual_a from user -> vaultA
        let transfer_a_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            user_token_a_acc.key,
            vault_a_acc.key,
            user_acc.key,
            &[],
            actual_a,
        )?;
        invoke(
            &transfer_a_ix,
            &[
                user_token_a_acc.clone(),
                vault_a_acc.clone(),
                user_acc.clone(),
                token_prog_acc.clone(),
            ],
        )?;

        // Transfer actual_b from user -> vaultB
        let transfer_b_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            user_token_b_acc.key,
            vault_b_acc.key,
            user_acc.key,
            &[],
            actual_b,
        )?;
        invoke(
            &transfer_b_ix,
            &[
                user_token_b_acc.clone(),
                vault_b_acc.clone(),
                user_acc.clone(),
                token_prog_acc.clone(),
            ],
        )?;

        // Mint LP to user
        let (sorted_mint_a, sorted_mint_b) =
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let sign_seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            pool_data.plugin_program_id.as_ref(),
            pool_data.plugin_state_pubkey.as_ref(),
            &[pool_data.bump],
        ];
        let mint_ix = spl_token::instruction::mint_to(
            token_prog_acc.key,
            &pool_data.lp_mint,
            user_lp_acc.key,
            pool_state_acc.key,
            &[],
            shares_to_mint,
        )?;
        invoke_signed(
            &mint_ix,
            &[
                lp_mint_acc.clone(),
                user_lp_acc.clone(),
                pool_state_acc.clone(),
                token_prog_acc.clone(),
            ],
            &[sign_seeds],
        )?;

        // Update total_lp_supply
        pool_data.total_lp_supply = pool_data
            .total_lp_supply
            .checked_add(shares_to_mint)
            .ok_or(PoolError::ArithmeticOverflow)?;
        pool_data.serialize(&mut *pool_state_acc.data.borrow_mut())?;

        Ok(())
    }

    fn process_remove_liquidity(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        amount_lp: u64,
    ) -> ProgramResult {
        msg!("Pool RemLiq: Processing");
        let acc_iter = &mut accounts.iter();
        let user_acc = next_account_info(acc_iter)?; // 0
        let pool_state_acc = next_account_info(acc_iter)?; // 1
        let vault_a_acc = next_account_info(acc_iter)?; // 2
        let vault_b_acc = next_account_info(acc_iter)?; // 3
        let lp_mint_acc = next_account_info(acc_iter)?; // 4
        let user_token_a_acc = next_account_info(acc_iter)?; // 5
        let user_token_b_acc = next_account_info(acc_iter)?; // 6
        let user_lp_acc = next_account_info(acc_iter)?; // 7
        let token_prog_acc = next_account_info(acc_iter)?; // 8
        let plugin_prog_acc = next_account_info(acc_iter)?; // 9
        let plugin_state_acc = next_account_info(acc_iter)?; // 10

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let mut pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;

        // --- PDA Re-derivation & Pool State Check ---
        let (expected_pda, _bump) = find_pool_address(
            program_id,
            &pool_data.token_mint_a,
            &pool_data.token_mint_b,
            &pool_data.plugin_program_id,
            &pool_data.plugin_state_pubkey,
        );
        if &expected_pda != pool_state_acc.key {
            return Err(PoolError::IncorrectPoolPDA.into());
        }

        // --- Account Key Checks vs Pool State ---
        if vault_a_acc.key != &pool_data.vault_a {
            return Err(PoolError::VaultMismatch.into());
        }
        if vault_b_acc.key != &pool_data.vault_b {
            return Err(PoolError::VaultMismatch.into());
        }
        if lp_mint_acc.key != &pool_data.lp_mint {
            return Err(PoolError::LpMintMismatch.into());
        }
        if plugin_prog_acc.key != &pool_data.plugin_program_id {
            return Err(PoolError::PluginProgramIdMismatch.into());
        }
        if plugin_state_acc.key != &pool_data.plugin_state_pubkey {
            return Err(PoolError::PluginStatePubkeyMismatch.into());
        }

        // --- Input Amount Check ---
        if amount_lp == 0 {
            return Err(PoolError::ZeroAmount.into());
        }
        // Check against current supply AFTER loading state
        if amount_lp > pool_data.total_lp_supply {
            // Use specific error? Or reuse InsufficientFunds?
            return Err(PoolError::InsufficientFunds.into());
        }

        // --- Account Data Validations ---
        validate_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
        validate_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;
        // Validate LP Mint (Properties only, supply should be > 0 here)
        let lp_mint_data = validate_mint_basic(lp_mint_acc)?;
        validate_lp_mint_properties(&lp_mint_data, &expected_pda)?;
        // Note: We already check amount_lp <= total_lp_supply earlier

        let _user_token_a_data =
            validate_token_account_basic(user_token_a_acc, user_acc.key, &pool_data.token_mint_a)?;
        let _user_token_b_data =
            validate_token_account_basic(user_token_b_acc, user_acc.key, &pool_data.token_mint_b)?;
        let user_lp_data =
            validate_token_account_basic(user_lp_acc, user_acc.key, &pool_data.lp_mint)?;
        if user_lp_data.amount < amount_lp {
            msg!(
                "User LP balance {} insufficient for burning {}",
                user_lp_data.amount,
                amount_lp
            );
            return Err(PoolError::InsufficientFunds.into());
        }
        // Plugin accounts are implicitly checked by CPI

        // --- Get Reserves (safe after validation) ---
        let vault_a_data = TokenAccount::unpack(&vault_a_acc.data.borrow())?;
        let vault_b_data = TokenAccount::unpack(&vault_b_acc.data.borrow())?;
        let reserve_a = vault_a_data.amount;
        let reserve_b = vault_b_data.amount;

        // plugin cpi -- Inlined
        let ix_data =
            constant_product_plugin::instruction::PluginInstruction::ComputeRemoveLiquidity {
                reserve_a,
                reserve_b,
                total_lp_supply: pool_data.total_lp_supply,
                lp_amount_burning: amount_lp,
            }
            .try_to_vec()?;
        let ix = solana_program::instruction::Instruction {
            program_id: pool_data.plugin_program_id,
            accounts: vec![
                // Mark as writable (implicit via accounts list), NOT signer (false)
                solana_program::instruction::AccountMeta::new(*plugin_state_acc.key, false),
            ],
            data: ix_data,
        };
        msg!("Pool: About to invoke plugin for RemoveLiquidity");
        invoke(
            &ix,
            &[
                plugin_prog_acc.clone(),
                plugin_state_acc.clone(), // Writable passed here
            ],
        )?;
        msg!("Pool: Plugin invoke successful (returned Ok)");

        let plugin_calc = PluginCalcResult::deserialize(&mut &plugin_state_acc.data.borrow()[..])?;
        let withdraw_a = plugin_calc.withdraw_a;
        let withdraw_b = plugin_calc.withdraw_b;

        // Burn user's LP - User must authorize this
        let burn_ix = spl_token::instruction::burn(
            token_prog_acc.key,
            user_lp_acc.key,    // Account to burn from
            &pool_data.lp_mint, // Mint of the token
            user_acc.key,       // Authority (owner of user_lp_acc)
            &[],                // No multisig signers needed
            amount_lp,
        )?;
        invoke(
            &burn_ix,
            &[
                user_lp_acc.clone(),    // Source account
                lp_mint_acc.clone(),    // Mint account
                user_acc.clone(),       // Authority account
                token_prog_acc.clone(), // Token program
            ],
            // No signers needed here, user signed the top-level tx
        )?;

        // Transfer out token A - Pool PDA must authorize this
        let (sorted_mint_a, sorted_mint_b) =
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let sign_seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            pool_data.plugin_program_id.as_ref(),
            pool_data.plugin_state_pubkey.as_ref(),
            &[pool_data.bump],
        ];
        let transfer_a_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            vault_a_acc.key,      // Source (Pool's vault)
            user_token_a_acc.key, // Destination (User's ATA)
            pool_state_acc.key,   // Authority (Pool PDA)
            &[],
            withdraw_a,
        )?;
        invoke_signed(
            &transfer_a_ix,
            &[
                vault_a_acc.clone(),
                user_token_a_acc.clone(),
                pool_state_acc.clone(), // Authority (Pool PDA)
                token_prog_acc.clone(),
            ],
            &[sign_seeds],
        )?;

        // Transfer out token B - Pool PDA must authorize this
        let transfer_b_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            vault_b_acc.key,      // Source (Pool's vault)
            user_token_b_acc.key, // Destination (User's ATA)
            pool_state_acc.key,   // Authority (Pool PDA)
            &[],
            withdraw_b,
        )?;
        invoke_signed(
            &transfer_b_ix,
            &[
                vault_b_acc.clone(),
                user_token_b_acc.clone(),
                pool_state_acc.clone(), // Authority (Pool PDA)
                token_prog_acc.clone(),
            ],
            &[sign_seeds],
        )?;

        // Update supply
        pool_data.total_lp_supply = pool_data
            .total_lp_supply
            .checked_sub(amount_lp)
            .ok_or(PoolError::ArithmeticOverflow)?;
        pool_data.serialize(&mut *pool_state_acc.data.borrow_mut())?;

        Ok(())
    }

    fn process_swap(
        program_id: &Pubkey,
        accounts: &[AccountInfo],
        amount_in: u64,
        min_out: u64,
    ) -> ProgramResult {
        msg!("Pool Swap: Processing");
        let acc_iter = &mut accounts.iter();
        let user_acc = next_account_info(acc_iter)?; // 0
        let pool_state_acc = next_account_info(acc_iter)?; // 1
        let vault_a_acc = next_account_info(acc_iter)?; // 2
        let vault_b_acc = next_account_info(acc_iter)?; // 3
        let user_src_acc = next_account_info(acc_iter)?; // 4
        let user_dst_acc = next_account_info(acc_iter)?; // 5
        let token_prog_acc = next_account_info(acc_iter)?; // 6
        let plugin_prog_acc = next_account_info(acc_iter)?; // 7
        let plugin_state_acc = next_account_info(acc_iter)?; // 8

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;
        if amount_in == 0 {
            return Err(PoolError::ZeroAmount.into());
        }
        if user_src_acc.key == user_dst_acc.key {
            msg!("User source and destination accounts cannot be the same");
            return Err(PoolError::InvalidArgument.into());
        }

        // --- PDA Re-derivation & Pool State Check ---
        let (expected_pda, _bump) = find_pool_address(
            program_id,
            &pool_data.token_mint_a,
            &pool_data.token_mint_b,
            &pool_data.plugin_program_id,
            &pool_data.plugin_state_pubkey,
        );
        if &expected_pda != pool_state_acc.key {
            return Err(PoolError::IncorrectPoolPDA.into());
        }

        // --- Account Key Checks vs Pool State ---
        if vault_a_acc.key != &pool_data.vault_a {
            return Err(PoolError::VaultMismatch.into());
        }
        if vault_b_acc.key != &pool_data.vault_b {
            return Err(PoolError::VaultMismatch.into());
        }
        // Swap doesn't use LP mint
        if plugin_prog_acc.key != &pool_data.plugin_program_id {
            return Err(PoolError::PluginProgramIdMismatch.into());
        }
        if plugin_state_acc.key != &pool_data.plugin_state_pubkey {
            return Err(PoolError::PluginStatePubkeyMismatch.into());
        }

        // --- Account Data Validations & Determine Swap Direction ---
        // Validate vaults first
        validate_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
        validate_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;

        // Validate user accounts and identify direction
        // Try validating src as Token A
        let src_mint = if let Ok(user_src_data) =
            validate_token_account_basic(user_src_acc, user_acc.key, &pool_data.token_mint_a)
        {
            // Source is Token A, Destination must be Token B
            let _user_dst_data =
                validate_token_account_basic(user_dst_acc, user_acc.key, &pool_data.token_mint_b)?;
            if user_src_data.amount < amount_in {
                return Err(PoolError::InsufficientFunds.into());
            }
            pool_data.token_mint_a
        } else if let Ok(user_src_data) =
            validate_token_account_basic(user_src_acc, user_acc.key, &pool_data.token_mint_b)
        {
            // Source is Token B, Destination must be Token A
            let _user_dst_data =
                validate_token_account_basic(user_dst_acc, user_acc.key, &pool_data.token_mint_a)?;
            if user_src_data.amount < amount_in {
                return Err(PoolError::InsufficientFunds.into());
            }
            pool_data.token_mint_b
        } else {
            // Source account matches neither mint A nor mint B, or fails basic validation
            msg!("Invalid user source token account or mint mismatch");
            return Err(PoolError::TokenMintMismatch.into());
        };

        // Identify reserve accounts based on src_mint
        let (reserve_in_acc, reserve_out_acc) = if src_mint == pool_data.token_mint_a {
            (vault_a_acc, vault_b_acc)
        } else {
            (vault_b_acc, vault_a_acc)
        };

        // --- Get Reserves (safe after validation) ---
        let reserve_in_data = TokenAccount::unpack(&reserve_in_acc.data.borrow())?;
        let reserve_out_data = TokenAccount::unpack(&reserve_out_acc.data.borrow())?;
        let r_in = reserve_in_data.amount;
        let r_out = reserve_out_data.amount;

        // plugin cpi -- Inlined
        let ix_data = constant_product_plugin::instruction::PluginInstruction::ComputeSwap {
            reserve_in: r_in,
            reserve_out: r_out,
            amount_in,
        }
        .try_to_vec()?;
        let ix = solana_program::instruction::Instruction {
            program_id: pool_data.plugin_program_id,
            accounts: vec![
                // Mark as writable (implicit), NOT signer
                solana_program::instruction::AccountMeta::new(*plugin_state_acc.key, false),
            ],
            data: ix_data,
        };
        invoke(
            &ix,
            &[
                plugin_prog_acc.clone(),  // Readonly
                plugin_state_acc.clone(), // Writable
            ],
        )?;

        let plugin_calc = PluginCalcResult::deserialize(&mut &plugin_state_acc.data.borrow()[..])?;
        let amount_out = plugin_calc.amount_out;
        if amount_out < min_out {
            return Err(PoolError::SlippageLimitExceeded.into());
        }
        if amount_out == 0 {
            return Err(PoolError::ZeroAmount.into());
        }

        // Transfer in from user -> reserve_in
        let transfer_in_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            user_src_acc.key,
            reserve_in_acc.key,
            user_acc.key,
            &[],
            amount_in,
        )?;
        invoke(
            &transfer_in_ix,
            // Revert back to passing all required accounts, including token_prog_acc
            &[
                user_src_acc.clone(),
                reserve_in_acc.clone(),
                user_acc.clone(),
                token_prog_acc.clone(), // Re-added
            ],
        )?;

        // Transfer out from reserve_out -> user_dst (pool signs)
        let (sorted_mint_a_key, sorted_mint_b_key) = // Store result of sorted()
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let sign_seeds = &[
            b"pool",
            sorted_mint_a_key.as_ref(), // Use variable
            sorted_mint_b_key.as_ref(), // Use variable
            pool_data.plugin_program_id.as_ref(),
            pool_data.plugin_state_pubkey.as_ref(),
            &[pool_data.bump],
        ];
        let transfer_out_ix = spl_token::instruction::transfer(
            token_prog_acc.key,
            reserve_out_acc.key,
            user_dst_acc.key,
            pool_state_acc.key,
            &[],
            amount_out,
        )?;
        invoke_signed(
            &transfer_out_ix,
            // Revert back to passing all required accounts, including token_prog_acc
            &[
                reserve_out_acc.clone(),
                user_dst_acc.clone(),
                pool_state_acc.clone(),
                token_prog_acc.clone(), // Re-added
            ],
            &[sign_seeds],
        )?;

        Ok(())
    }
}

/// Utility: sort two pubkeys consistently
fn sorted(a: &Pubkey, b: &Pubkey) -> (Pubkey, Pubkey) {
    if a < b {
        (*a, *b)
    } else {
        (*b, *a)
    }
}
