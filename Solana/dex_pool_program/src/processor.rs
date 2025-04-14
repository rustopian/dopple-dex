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
    program_error::ProgramError,
};
use spl_token::state::Account as TokenAccount;

use crate::error::PoolError;
use crate::instruction::PoolInstruction;
use crate::state::PoolState;
use crate::{NATIVE_MINT, constants};
use crate::pda::{
    find_pool_address,
    find_sol_vault_address,
    get_pool_seeds,
    validate_executable,
    validate_mint_basic,
    validate_lp_mint_properties,
    validate_lp_mint_zero_supply,
    validate_program_id,
    validate_rent_exemption,
    validate_spl_pool_vault,
    validate_sol_pool_vault,
    validate_spl_token_account,
    validate_user_sol_account,
    SOL_VAULT_PREFIX,
};

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
        let vault_a_acc = next_account_info(acc_iter)?; // 2 (Always passed)
        let vault_b_acc = next_account_info(acc_iter)?; // 3 (Always passed)
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

        // Validate mints (basic checks + rent)
        let mint_a_is_native = mint_a_acc.key == &NATIVE_MINT;
        let mint_b_is_native = mint_b_acc.key == &NATIVE_MINT;

        if mint_a_is_native && mint_b_is_native {
            msg!("Error: Both mints cannot be native SOL");
            return Err(PoolError::InvalidArgument.into()); // Or a more specific error
        }

        let _mint_a_data = validate_mint_basic(mint_a_acc)?;
        if !mint_a_is_native {
            validate_rent_exemption(mint_a_acc, &rent)?;
        }
        let _mint_b_data = validate_mint_basic(mint_b_acc)?;
        if !mint_b_is_native {
            validate_rent_exemption(mint_b_acc, &rent)?;
        }

        // --- PDA Derivation & Validation ---
        msg!("Pool Init: Deriving pool PDA...");
        let (sort_mint_a, sort_mint_b) = if mint_a_acc.key < mint_b_acc.key {
            (mint_a_acc.key, mint_b_acc.key)
        } else {
            (mint_b_acc.key, mint_a_acc.key)
        };
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

        // --- LP Mint & Vault Account Validation ---
        msg!("Pool Init: Validating Vaults & LP Mint...");
        // LP Mint (Always SPL)
        let lp_mint_data_option = validate_mint_basic(lp_mint_acc)?;
        let lp_mint_data = lp_mint_data_option.ok_or(PoolError::InvalidMint)?;
        validate_lp_mint_properties(&lp_mint_data, &expected_pool_pda)?;
        validate_lp_mint_zero_supply(&lp_mint_data)?;
        validate_rent_exemption(lp_mint_acc, &rent)?;

        // Vault A Validation & Creation
        if mint_a_is_native {
            let (expected_sol_vault_pda, sol_vault_a_bump) = find_sol_vault_address(&expected_pool_pda, program_id);
            // Ensure the account passed matches the derived address
            if vault_a_acc.key != &expected_sol_vault_pda {
                msg!("Invalid SOL Vault A account key provided. Expected {}, got {}",
                    expected_sol_vault_pda, vault_a_acc.key);
                return Err(PoolError::IncorrectPoolPDA.into());
            }
            // Now create it using invoke_signed
            msg!("Creating SOL Vault A PDA: {}", expected_sol_vault_pda);
            let sol_vault_a_signer_seeds = &[SOL_VAULT_PREFIX, expected_pool_pda.as_ref(), &[sol_vault_a_bump]];
            invoke_signed(
                &system_instruction::create_account(
                    payer_acc.key,
                    &expected_sol_vault_pda,
                    rent.minimum_balance(0),
                    0,
                    program_id,
                ),
                // Accounts: Payer, New Account (Info passed from client), System Program
                &[payer_acc.clone(), vault_a_acc.clone(), system_acc.clone()],
                &[sol_vault_a_signer_seeds],
            )?;
            // Validate the created account's owner and data (optional, but good practice)
            validate_sol_pool_vault(vault_a_acc, &expected_sol_vault_pda, program_id)?;
        } else {
            validate_spl_pool_vault(vault_a_acc, &expected_pool_pda, mint_a_acc.key)?;
        }

        // Vault B Validation & Creation
        if mint_b_is_native {
            let (expected_sol_vault_pda, sol_vault_b_bump) = find_sol_vault_address(&expected_pool_pda, program_id);
            // Ensure the account passed matches the derived address
             if vault_b_acc.key != &expected_sol_vault_pda {
                msg!("Invalid SOL Vault B account key provided. Expected {}, got {}",
                    expected_sol_vault_pda, vault_b_acc.key);
                return Err(PoolError::IncorrectPoolPDA.into());
            }
            // Now create it using invoke_signed
            msg!("Creating SOL Vault B PDA: {}", expected_sol_vault_pda);
            let sol_vault_b_signer_seeds = &[SOL_VAULT_PREFIX, expected_pool_pda.as_ref(), &[sol_vault_b_bump]];
            invoke_signed(
                &system_instruction::create_account(
                    payer_acc.key,
                    &expected_sol_vault_pda,
                    rent.minimum_balance(0),
                    0,
                    program_id,
                ),
                 // Accounts: Payer, New Account (Info passed from client), System Program
                &[payer_acc.clone(), vault_b_acc.clone(), system_acc.clone()],
                &[sol_vault_b_signer_seeds],
            )?;
             // Validate the created account's owner and data (optional, but good practice)
            validate_sol_pool_vault(vault_b_acc, &expected_sol_vault_pda, program_id)?;
        } else {
            validate_spl_pool_vault(vault_b_acc, &expected_pool_pda, mint_b_acc.key)?;
        }

        msg!("Pool Init: Vaults validated/created.");

        // --- Pool State Account Creation & State Initialization ---
        msg!("Pool Init: Creating Pool State Account...");

        // Calculate PoolState size
        let pool_state_size = borsh::to_vec(&PoolState {
            token_mint_a: *mint_a_acc.key,
            token_mint_b: *mint_b_acc.key,
            vault_a: *vault_a_acc.key,
            vault_b: *vault_b_acc.key,
            lp_mint: *lp_mint_acc.key,
            total_lp_supply: 0,
            bump,
            plugin_program_id: *plugin_prog_acc.key,
            plugin_state_pubkey: *plugin_state_acc.key,
        })?.len();
        let needed_lamports = rent.minimum_balance(pool_state_size);

        let pool_pda_signer_seeds = &[
            b"pool",
            sort_mint_a.as_ref(),
            sort_mint_b.as_ref(),
            plugin_prog_acc.key.as_ref(),
            plugin_state_acc.key.as_ref(),
            &[bump],
        ];

        invoke_signed(
             &system_instruction::create_account(
                 payer_acc.key,
                 pool_state_acc.key,
                 needed_lamports,
                 pool_state_size as u64,
                 program_id,
             ),
             &[payer_acc.clone(), pool_state_acc.clone(), system_acc.clone()],
             &[pool_pda_signer_seeds],
         )?;

        msg!("Pool Init: Writing initial state...");
        // Serialize the final state into the created account
        let final_pool_data = PoolState {
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
        final_pool_data.serialize(&mut *pool_state_acc.data.borrow_mut())?;

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
        let system_acc = next_account_info(acc_iter)?; // 11

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let mut pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;
        validate_program_id(system_acc, &solana_program::system_program::id())?;

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
        let mint_a_is_native = pool_data.token_mint_a == NATIVE_MINT;
        let mint_b_is_native = pool_data.token_mint_b == NATIVE_MINT;

        if mint_a_is_native {
            validate_sol_pool_vault(vault_a_acc, &pool_data.vault_a, program_id)?;
            validate_user_sol_account(user_token_a_acc, user_acc.key, true, false)?; // Signer=true if transferring FROM user
        } else {
            validate_spl_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
            let _ = validate_spl_token_account(user_token_a_acc, user_acc.key, &pool_data.token_mint_a)?;
        }
        if mint_b_is_native {
            validate_sol_pool_vault(vault_b_acc, &pool_data.vault_b, program_id)?;
            validate_user_sol_account(user_token_b_acc, user_acc.key, true, false)?; // Signer=true if transferring FROM user
        } else {
            validate_spl_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;
            let _ = validate_spl_token_account(user_token_b_acc, user_acc.key, &pool_data.token_mint_b)?;
        }

        // Validate LP Mint (Properties only, supply can be non-zero)
        let lp_mint_data_option = validate_mint_basic(lp_mint_acc)?;
        let lp_mint_data = lp_mint_data_option.ok_or(PoolError::InvalidMint)?;
        validate_lp_mint_properties(&lp_mint_data, &expected_pda)?;

        // Validate user LP account (Always SPL)
        let _user_lp_data = validate_spl_token_account(
            user_lp_acc,
            user_acc.key,
            &pool_data.lp_mint,
        )?;
        // Plugin accounts are implicitly checked by CPI

        // --- Get Reserves (safe after validation) ---
        let reserve_a = if pool_data.token_mint_a == NATIVE_MINT {
            // Subtract rent reserve? For simplicity, let's assume the full balance is usable for now.
            // A production system might need `vault_a_acc.lamports().checked_sub(rent.minimum_balance(0)).unwrap_or(0)`
            vault_a_acc.lamports()
        } else {
            TokenAccount::unpack(&vault_a_acc.data.borrow())?.amount
        };
        let reserve_b = if pool_data.token_mint_b == NATIVE_MINT {
            vault_b_acc.lamports()
        } else {
            TokenAccount::unpack(&vault_b_acc.data.borrow())?.amount
        };

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

        // --- Perform Transfers (Conditional) ---
        // Transfer actual_a from user -> vaultA
        if pool_data.token_mint_a == NATIVE_MINT {
            invoke(
                &system_instruction::transfer(user_acc.key, vault_a_acc.key, actual_a),
                &[user_acc.clone(), vault_a_acc.clone(), system_acc.clone()], // System Program needed
            )?;
        } else {
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
        }

        // Transfer actual_b from user -> vaultB
        if pool_data.token_mint_b == NATIVE_MINT {
             invoke(
                &system_instruction::transfer(user_acc.key, vault_b_acc.key, actual_b),
                &[user_acc.clone(), vault_b_acc.clone(), system_acc.clone()], // System Program needed
            )?;
        } else {
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
        }

        // Mint LP to user (Always SPL)
        let (sorted_mint_a_key, sorted_mint_b_key) = // Store result of sorted()
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let sign_seeds = &[
            b"pool",
            sorted_mint_a_key.as_ref(),
            sorted_mint_b_key.as_ref(),
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
        let system_acc = next_account_info(acc_iter)?; // 11
        let rent_acc = next_account_info(acc_iter)?; // 12

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let mut pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;
        validate_program_id(system_acc, &solana_program::system_program::id())?;
        validate_program_id(rent_acc, &solana_program::sysvar::rent::id())?;
        let rent = Rent::from_account_info(rent_acc)?;

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
        if amount_lp > pool_data.total_lp_supply {
            return Err(PoolError::InsufficientFunds.into());
        }

        // --- Account Data Validations ---
        let mut user_token_a_is_sol = false;
        let mut user_token_b_is_sol = false;

        if pool_data.token_mint_a == NATIVE_MINT {
            validate_sol_pool_vault(vault_a_acc, &pool_data.vault_a, program_id)?;
            validate_user_sol_account(user_token_a_acc, user_acc.key, false, true)?;
            user_token_a_is_sol = true;
        } else {
            validate_spl_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
            let _ = validate_spl_token_account(user_token_a_acc, user_acc.key, &pool_data.token_mint_a)?;
        }
        if pool_data.token_mint_b == NATIVE_MINT {
            validate_sol_pool_vault(vault_b_acc, &pool_data.vault_b, program_id)?;
            validate_user_sol_account(user_token_b_acc, user_acc.key, false, true)?;
            user_token_b_is_sol = true;
        } else {
            validate_spl_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;
            let _ = validate_spl_token_account(user_token_b_acc, user_acc.key, &pool_data.token_mint_b)?;
        }

        // Validate LP Mint (Properties only, supply should be > 0 here)
        let lp_mint_data_option = validate_mint_basic(lp_mint_acc)?;
        let _lp_mint_data = lp_mint_data_option.ok_or(PoolError::InvalidMint)?;
        validate_lp_mint_properties(&_lp_mint_data, &expected_pda)?;

        // Validate user LP account (Always SPL)
        let user_lp_data = validate_spl_token_account(
            user_lp_acc,
            user_acc.key,
            &pool_data.lp_mint,
        )?;
        if user_lp_data.amount < amount_lp {
            msg!("User LP balance {} insufficient for burning {}", user_lp_data.amount, amount_lp);
            return Err(PoolError::InsufficientFunds.into());
        }
        // Plugin accounts are implicitly checked by CPI

        // --- Get Reserves (safe after validation) ---
         let reserve_a = if pool_data.token_mint_a == NATIVE_MINT {
            vault_a_acc.lamports()
        } else {
            TokenAccount::unpack(&vault_a_acc.data.borrow())?.amount
        };
        let reserve_b = if pool_data.token_mint_b == NATIVE_MINT {
            vault_b_acc.lamports()
        } else {
            TokenAccount::unpack(&vault_b_acc.data.borrow())?.amount
        };

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

        // Burn user's LP (Always SPL)
        let burn_ix = spl_token::instruction::burn(
            token_prog_acc.key,
            user_lp_acc.key,    // Account to burn from
            &pool_data.lp_mint, // Mint of the token
            user_acc.key,       // Authority (owner of user_lp_acc)
            &[],                // (no multisig signers)
            amount_lp,
        )?;
        invoke(
            &burn_ix,
            &[
                user_lp_acc.clone(),
                lp_mint_acc.clone(),
                user_acc.clone(),
                token_prog_acc.clone(),
            ],
        )?;

        // --- Perform Transfers Out (Conditional) ---
        let (sorted_mint_a_key, sorted_mint_b_key) = // Store result
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let pool_signer_seeds = &[ // Base seeds for pool PDA signing
            b"pool",
            sorted_mint_a_key.as_ref(), // Use variable
            sorted_mint_b_key.as_ref(), // Use variable
            pool_data.plugin_program_id.as_ref(),
            pool_data.plugin_state_pubkey.as_ref(),
            &[pool_data.bump],
        ];

        // Transfer out token A
        if user_token_a_is_sol {
            // Check sufficient lamports in vault (leave rent minimum)
            let rent_minimum = rent.minimum_balance(0);
            if vault_a_acc.lamports().saturating_sub(rent_minimum) < withdraw_a {
                 return Err(PoolError::InsufficientFunds.into());
            }
            invoke_signed(
                &system_instruction::transfer(pool_state_acc.key, user_token_a_acc.key, withdraw_a),
                &[pool_state_acc.clone(), user_token_a_acc.clone(), system_acc.clone()],
                &[pool_signer_seeds],
            )?;
        } else {
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
                    pool_state_acc.clone(),
                    token_prog_acc.clone(),
                ],
                &[pool_signer_seeds],
            )?;
        }

        // Transfer out token B
        if user_token_b_is_sol {
             let rent_minimum = rent.minimum_balance(0);
            if vault_b_acc.lamports().saturating_sub(rent_minimum) < withdraw_b {
                 return Err(PoolError::InsufficientFunds.into());
            }
             invoke_signed(
                &system_instruction::transfer(pool_state_acc.key, user_token_b_acc.key, withdraw_b),
                &[pool_state_acc.clone(), user_token_b_acc.clone(), system_acc.clone()],
                &[pool_signer_seeds],
            )?;
        } else {
            let transfer_b_ix = spl_token::instruction::transfer(
                token_prog_acc.key,
                vault_b_acc.key,
                user_token_b_acc.key,
                pool_state_acc.key,
                &[],
                withdraw_b,
            )?;
            invoke_signed(
                &transfer_b_ix,
                &[
                    vault_b_acc.clone(),
                    user_token_b_acc.clone(),
                    pool_state_acc.clone(),
                    token_prog_acc.clone(),
                ],
                &[pool_signer_seeds],
            )?;
        }

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
        let system_acc = next_account_info(acc_iter)?; // 9
        let rent_acc = next_account_info(acc_iter)?; // 10

        // --- Load State & Basic Checks ---
        if !user_acc.is_signer {
            return Err(PoolError::MissingRequiredSignature.into());
        }
        let pool_data = PoolState::try_from_slice(&pool_state_acc.data.borrow())?;
        validate_program_id(token_prog_acc, &spl_token::id())?;
        validate_program_id(system_acc, &solana_program::system_program::id())?;
        validate_program_id(rent_acc, &solana_program::sysvar::rent::id())?;
        let rent = Rent::from_account_info(rent_acc)?;
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
        if plugin_prog_acc.key != &pool_data.plugin_program_id {
            return Err(PoolError::PluginProgramIdMismatch.into());
        }
        if plugin_state_acc.key != &pool_data.plugin_state_pubkey {
            return Err(PoolError::PluginStatePubkeyMismatch.into());
        }

        // --- Account Data Validations & Determine Swap Direction ---
        let mint_a_is_native = pool_data.token_mint_a == NATIVE_MINT;
        let mint_b_is_native = pool_data.token_mint_b == NATIVE_MINT;

        // Validate vaults first
        if mint_a_is_native {
            validate_sol_pool_vault(vault_a_acc, &pool_data.vault_a, program_id)?;
        } else {
            validate_spl_pool_vault(vault_a_acc, &expected_pda, &pool_data.token_mint_a)?;
        }
        if mint_b_is_native {
            validate_sol_pool_vault(vault_b_acc, &pool_data.vault_b, program_id)?;
        } else {
            validate_spl_pool_vault(vault_b_acc, &expected_pda, &pool_data.token_mint_b)?;
        }

        // Validate user accounts and identify direction
        let (src_mint, reserve_in_acc, reserve_out_acc) = if !mint_a_is_native && !mint_b_is_native {
             // Standard SPL -> SPL swap
            if let Ok(user_src_data) = validate_spl_token_account(user_src_acc, user_acc.key, &pool_data.token_mint_a) {
                let _ = validate_spl_token_account(user_dst_acc, user_acc.key, &pool_data.token_mint_b)?;
                if user_src_data.amount < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                (pool_data.token_mint_a, vault_a_acc, vault_b_acc)
            } else if let Ok(user_src_data) = validate_spl_token_account(user_src_acc, user_acc.key, &pool_data.token_mint_b) {
                let _ = validate_spl_token_account(user_dst_acc, user_acc.key, &pool_data.token_mint_a)?;
                if user_src_data.amount < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                (pool_data.token_mint_b, vault_b_acc, vault_a_acc)
            } else {
                msg!("Invalid SPL user source token account or mint mismatch");
                return Err(PoolError::TokenMintMismatch.into());
            }
        } else if mint_a_is_native { // Token A is SOL, Token B is SPL
            if let Ok(()) = validate_user_sol_account(user_src_acc, user_acc.key, true, false) { // Check user SOL src
                let _ = validate_spl_token_account(user_dst_acc, user_acc.key, &pool_data.token_mint_b)?; // Dest must be SPL B
                if user_src_acc.lamports() < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                (pool_data.token_mint_a, vault_a_acc, vault_b_acc)
            } else if let Ok(user_src_data) = validate_spl_token_account(user_src_acc, user_acc.key, &pool_data.token_mint_b) { // Check user SPL B src
                let _ = validate_user_sol_account(user_dst_acc, user_acc.key, false, true)?; // Dest must be SOL A
                if user_src_data.amount < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                 (pool_data.token_mint_b, vault_b_acc, vault_a_acc)
            } else {
                 msg!("Invalid user source account (SOL A / SPL B pool)");
                 return Err(PoolError::TokenMintMismatch.into());
            }
        } else { // Token B is SOL, Token A is SPL (mint_b_is_native must be true)
             if let Ok(user_src_data) = validate_spl_token_account(user_src_acc, user_acc.key, &pool_data.token_mint_a) { // Check user SPL A src
                 let _ = validate_user_sol_account(user_dst_acc, user_acc.key, false, true)?; // Dest must be SOL B
                 if user_src_data.amount < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                 (pool_data.token_mint_a, vault_a_acc, vault_b_acc)
             } else if let Ok(()) = validate_user_sol_account(user_src_acc, user_acc.key, true, false) { // Check user SOL B src
                 let _ = validate_spl_token_account(user_dst_acc, user_acc.key, &pool_data.token_mint_a)?; // Dest must be SPL A
                 if user_src_acc.lamports() < amount_in { return Err(PoolError::InsufficientFunds.into()); }
                 (pool_data.token_mint_b, vault_b_acc, vault_a_acc)
             } else {
                  msg!("Invalid user source account (SPL A / SOL B pool)");
                  return Err(PoolError::TokenMintMismatch.into());
             }
        };

        // --- Get Reserves (safe after validation) ---
        let r_in = if src_mint == NATIVE_MINT {
            reserve_in_acc.lamports()
        } else {
            TokenAccount::unpack(&reserve_in_acc.data.borrow())?.amount
        };
        let r_out = if reserve_out_acc.key == &pool_data.vault_a { // Check which vault is the out vault
            if mint_a_is_native {
                reserve_out_acc.lamports()
            } else {
                 TokenAccount::unpack(&reserve_out_acc.data.borrow())?.amount
            }
        } else { // reserve_out_acc must be vault_b
            if mint_b_is_native {
                reserve_out_acc.lamports()
            } else {
                 TokenAccount::unpack(&reserve_out_acc.data.borrow())?.amount
            }
        };

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
                solana_program::instruction::AccountMeta::new(*plugin_state_acc.key, false),
            ],
            data: ix_data,
        };
        invoke(
            &ix,
            &[
                plugin_prog_acc.clone(),
                plugin_state_acc.clone(),
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

        // Transfer In: User -> Pool Vault
        if src_mint == NATIVE_MINT {
             invoke(
                &system_instruction::transfer(user_acc.key, reserve_in_acc.key, amount_in),
                &[user_acc.clone(), reserve_in_acc.clone(), system_acc.clone()],
            )?;
        } else {
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
                &[
                    user_src_acc.clone(),
                    reserve_in_acc.clone(),
                    user_acc.clone(),
                    token_prog_acc.clone(),
                ],
            )?;
        }

        // Transfer Out: Pool Vault -> User
        let (sorted_mint_a_key, sorted_mint_b_key) = // Store result
            sorted(&pool_data.token_mint_a, &pool_data.token_mint_b);
        let pool_signer_seeds = &[ // Base seeds for pool PDA signing
            b"pool",
            sorted_mint_a_key.as_ref(), // Use variable
            sorted_mint_b_key.as_ref(), // Use variable
            pool_data.plugin_program_id.as_ref(),
            pool_data.plugin_state_pubkey.as_ref(),
            &[pool_data.bump],
        ];
        let reserve_out_is_sol = (reserve_out_acc.key == vault_a_acc.key && mint_a_is_native) ||
                                   (reserve_out_acc.key == vault_b_acc.key && mint_b_is_native);

        if reserve_out_is_sol {
            // Check sufficient lamports in vault (leave rent minimum)
            let rent_minimum = rent.minimum_balance(0); // Need Rent sysvar!
             if reserve_out_acc.lamports().saturating_sub(rent_minimum) < amount_out {
                 return Err(PoolError::InsufficientFunds.into());
            }
             invoke_signed(
                &system_instruction::transfer(pool_state_acc.key, user_dst_acc.key, amount_out),
                &[pool_state_acc.clone(), user_dst_acc.clone(), system_acc.clone()],
                &[pool_signer_seeds],
            )?;
        } else {
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
                &[
                    reserve_out_acc.clone(),
                    user_dst_acc.clone(),
                    pool_state_acc.clone(),
                    token_prog_acc.clone(),
                ],
                &[pool_signer_seeds],
            )?;
        }

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
