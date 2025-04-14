use {
    borsh::{BorshDeserialize, BorshSerialize},
    dex_pool_program::instruction::PoolInstruction,
    // std::path::{Path, PathBuf}, // Removed unused Path, PathBuf
    dex_pool_program::processor::PluginCalcResult,
    dex_pool_program::state::PoolState,
    litesvm::{
        types::{FailedTransactionMetadata, TransactionMetadata},
        LiteSVM,
    },
    solana_sdk::{
        // account::Account, // Removed, covered by Pack
        instruction::{AccountMeta, Instruction},
        // Use Pack from the spl_token re-export
        // program_pack::Pack, // Old import
        pubkey::Pubkey,
        signature::Signer,
        signer::keypair::Keypair,
        system_program,
        sysvar::{self, rent::Rent}, // Removed unused Sysvar trait
        transaction::Transaction,
        message::Message,
    },
    spl_associated_token_account,
    spl_memo,                                              // Added import for spl-memo
    spl_token::{self, solana_program::program_pack::Pack}, // Use Pack from here for spl_token::state::Account
    std::env,                                              // Keep for current_dir
    std::error::Error,
    std::mem::size_of,
};

// REMOVED Obsolete TODO comment block

// REMOVED Constants for paths
// const DEX_SO_PATH: &str = "target/deploy/dex_pool_program.so";
// const PLUGIN_SO_PATH: &str = "target/deploy/constant_product_plugin.so";

// Define a struct to hold the common setup elements
struct TestSetup {
    svm: LiteSVM,
    payer: Keypair,
    mint_authority: Keypair,
    dex_pid: Pubkey,
    plugin_pid: Pubkey,
    mint_a: Pubkey,
    mint_b: Pubkey,
    lp_mint: Pubkey,
    plugin_state_pk: Pubkey,
    pool_pda: Pubkey,
    pool_bump: u8,
    vault_a_pk: Pubkey,
    vault_b_pk: Pubkey,
}

// Helper function to handle litesvm errors (Generic over Debug error type)
fn map_litesvm_err<T, E: std::fmt::Debug>(res: Result<T, E>) -> Result<T, Box<dyn Error>> {
    res.map_err(|e| Box::<dyn Error>::from(format!("LiteSVM Error: {:?}", e)))
}

// Helper function to create mint accounts
fn create_mint(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint_authority: &Pubkey,
) -> Result<Keypair, Box<dyn std::error::Error>> {
    let mint_kp = Keypair::new();
    let mint_pk = mint_kp.pubkey();
    // get_sysvar returns Rent directly, no need for `?`
    let rent = svm.get_sysvar::<Rent>();
    // Use Pack::LEN
    let mint_rent = rent.minimum_balance(spl_token::state::Mint::LEN);

    let create_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &mint_pk,
        mint_rent,
        // Use Pack::LEN
        spl_token::state::Mint::LEN as u64,
        &spl_token::id(),
    );

    let init_ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint_pk,
        mint_authority,
        None, // freeze authority
        0,    // decimals
    )?;

    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[payer, &mint_kp], // Mint keypair also needs to sign for creation
        svm.latest_blockhash(),
    );

    // Map the error
    map_litesvm_err(svm.send_transaction(tx))?;
    Ok(mint_kp)
}

// Helper function to create a user ATA
fn create_user_ata(
    svm: &mut LiteSVM,
    payer: &Keypair,
    user: &Pubkey,
    mint: &Pubkey,
) -> Result<Pubkey, Box<dyn Error>> {
    let ata_pk = spl_associated_token_account::get_associated_token_address(user, mint);
    let ix = spl_associated_token_account::instruction::create_associated_token_account(
        &payer.pubkey(),
        user,
        mint,
        &spl_token::id(),
    );
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer],
        svm.latest_blockhash(),
    );
    map_litesvm_err(svm.send_transaction(tx))?;
    Ok(ata_pk)
}

// Helper function to mint tokens to an ATA
fn mint_to_ata(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint_authority: &Keypair,
    mint: &Pubkey,
    ata: &Pubkey,
    amount: u64,
) -> Result<(), Box<dyn Error>> {
    let ix = spl_token::instruction::mint_to(
        &spl_token::id(),
        mint,
        ata,
        &mint_authority.pubkey(),
        &[&mint_authority.pubkey()],
        amount,
    )?;
    let tx = Transaction::new_signed_with_payer(
        &[ix],
        Some(&payer.pubkey()),
        &[payer, mint_authority], // Need mint authority to sign
        svm.latest_blockhash(),
    );
    map_litesvm_err(svm.send_transaction(tx))?;
    Ok(())
}

// Helper to get token account balance
fn get_token_balance(svm: &LiteSVM, ata_pk: &Pubkey) -> u64 {
    svm.get_account(ata_pk)
        .map(|acc| spl_token::state::Account::unpack(&acc.data).unwrap().amount)
        .unwrap_or(0)
}

// NEW HELPER: Setup user and their ATAs
fn setup_user_accounts(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint_a: &Pubkey,
    mint_b: &Pubkey,
    mint_lp: &Pubkey,
) -> Result<(Keypair, Pubkey, Pubkey, Pubkey), Box<dyn Error>> {
    let user_kp = Keypair::new();
    let user_pk = user_kp.pubkey();
    map_litesvm_err(svm.airdrop(&user_pk, 1_000_000_000))?; // 1 SOL
    let user_ata_a = create_user_ata(svm, payer, &user_pk, mint_a)?;
    let user_ata_b = create_user_ata(svm, payer, &user_pk, mint_b)?;
    let user_ata_lp = create_user_ata(svm, payer, &user_pk, mint_lp)?;
    Ok((user_kp, user_ata_a, user_ata_b, user_ata_lp))
}

// NEW HELPER: Add liquidity transaction (assumes user has funds in source ATAs)
fn execute_add_liquidity(
    setup: &mut TestSetup,
    user_kp: &Keypair,
    user_ata_a: &Pubkey,
    user_ata_b: &Pubkey,
    user_ata_lp: &Pubkey,
    amount_a: u64,
    amount_b: u64,
) -> Result<(), Box<dyn Error>> {
    let add_liq_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(user_kp.pubkey(), true), // User is signer
            AccountMeta::new(setup.pool_pda, false),
            AccountMeta::new(setup.vault_a_pk, false),
            AccountMeta::new(setup.vault_b_pk, false),
            AccountMeta::new(setup.lp_mint, false),
            AccountMeta::new(*user_ata_a, false), // User's source ATA A
            AccountMeta::new(*user_ata_b, false), // User's source ATA B
            AccountMeta::new(*user_ata_lp, false), // User's dest LP ATA
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(setup.plugin_pid, false),
            AccountMeta::new(setup.plugin_state_pk, false),
        ],
        data: PoolInstruction::AddLiquidity { amount_a, amount_b }.try_to_vec()?,
    };
    let tx = Transaction::new_signed_with_payer(
        &[add_liq_ix],
        Some(&setup.payer.pubkey()), // Setup payer pays fees
        &[&setup.payer, user_kp],    // Payer + User sign
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(tx))?;
    Ok(())
}

// NEW HELPER: Get Pool State
fn get_pool_state(svm: &LiteSVM, pool_pda: &Pubkey) -> Result<PoolState, Box<dyn Error>> {
    let pool_account = svm
        .get_account(pool_pda)
        .ok_or_else(|| Box::<dyn Error>::from(format!("Pool account {} not found", pool_pda)))?;
    PoolState::try_from_slice(&pool_account.data)
        .map_err(|e| Box::<dyn Error>::from(format!("Failed to deserialize PoolState: {}", e)))
}

// The main setup function
fn setup_test_environment() -> Result<TestSetup, Box<dyn Error>> {
    // Generate dynamic program IDs for this test run
    let dex_pid = Pubkey::new_unique();
    let plugin_pid = Pubkey::new_unique();
    println!("Using DEX Program ID: {}", dex_pid);
    println!("Using Plugin Program ID: {}", plugin_pid);

    // Construct absolute paths to SO files from WORKSPACE root
    let current_dir = env::current_dir()?;
    let workspace_root = current_dir.parent().ok_or_else(|| {
        Box::<dyn Error>::from("Failed to get parent directory of current test execution dir")
    })?;
    let dex_so_path = workspace_root
        .join("target")
        .join("deploy")
        .join("dex_pool_program.so");
    let plugin_so_path = workspace_root
        .join("target")
        .join("deploy")
        .join("constant_product_plugin.so");
    println!("Attempting to load DEX SO from: {}", dex_so_path.display());
    println!(
        "Attempting to load Plugin SO from: {}",
        plugin_so_path.display()
    );

    // 1. Initialize SVM
    let mut svm = LiteSVM::new();

    // 2. Load programs using correct absolute paths
    map_litesvm_err(svm.add_program_from_file(dex_pid, &dex_so_path))?;
    map_litesvm_err(svm.add_program_from_file(plugin_pid, &plugin_so_path))?;

    // 3. Create keypairs
    let payer = Keypair::new();
    let mint_authority = Keypair::new(); // Authority for creating mints/vaults

    // 4. Airdrop (map error)
    map_litesvm_err(svm.airdrop(&payer.pubkey(), 10_000_000_000))?; // 10 SOL
    map_litesvm_err(svm.airdrop(&mint_authority.pubkey(), 1_000_000_000))?; // 1 SOL

    // 5. Create mints
    let mint_a_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;
    let mint_b_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;
    let lp_mint_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?; // Pool PDA will be mint authority later

    let mint_a = mint_a_kp.pubkey();
    let mint_b = mint_b_kp.pubkey();
    let lp_mint = lp_mint_kp.pubkey();

    // Sort mints for PDA derivation
    let (sorted_mint_a, sorted_mint_b) = if mint_a < mint_b {
        (mint_a, mint_b)
    } else {
        (mint_b, mint_a)
    };

    // 6. Create Plugin State Account
    let plugin_state_kp = Keypair::new();
    let plugin_state_pk = plugin_state_kp.pubkey();
    // Calculate size based on the struct the plugin writes
    let plugin_state_size = size_of::<PluginCalcResult>();
    println!("Plugin State Account Size: {}", plugin_state_size);
    let rent = svm.get_sysvar::<Rent>();
    let plugin_state_rent = rent.minimum_balance(plugin_state_size);

    let create_plugin_state_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &plugin_state_pk,
        plugin_state_rent,
        plugin_state_size as u64, // Use calculated size
        &plugin_pid,
    );
    let tx_plugin_state = Transaction::new_signed_with_payer(
        &[create_plugin_state_ix],
        Some(&payer.pubkey()),
        &[&payer, &plugin_state_kp],
        svm.latest_blockhash(),
    );
    map_litesvm_err(svm.send_transaction(tx_plugin_state))?;
    println!("Created Plugin State: {}", plugin_state_pk);

    // 7. Derive PDAs
    let (pool_pda, pool_bump) = Pubkey::find_program_address(
        &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_pid.as_ref(),
            plugin_state_pk.as_ref(),
        ],
        &dex_pid,
    );
    println!("Derived Pool PDA: {}, Bump: {}", pool_pda, pool_bump);

    let vault_a_pk = spl_associated_token_account::get_associated_token_address(&pool_pda, &mint_a);
    let vault_b_pk = spl_associated_token_account::get_associated_token_address(&pool_pda, &mint_b);
    println!("Derived Vault A ATA: {}", vault_a_pk);
    println!("Derived Vault B ATA: {}", vault_b_pk);

    // Create Vault A and Vault B using the ATA instruction
    let create_ata_a_ix =
        spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),
            &pool_pda,
            &mint_a,
            &spl_token::id(),
        );
    let create_ata_b_ix =
        spl_associated_token_account::instruction::create_associated_token_account(
            &payer.pubkey(),
            &pool_pda,
            &mint_b,
            &spl_token::id(),
        );
    let set_lp_auth_ix = spl_token::instruction::set_authority(
        &spl_token::id(),
        &lp_mint,
        Some(&pool_pda),
        spl_token::instruction::AuthorityType::MintTokens,
        &mint_authority.pubkey(),
        &[&mint_authority.pubkey()],
    )?;

    let setup_tx = Transaction::new_signed_with_payer(
        &[create_ata_a_ix, create_ata_b_ix, set_lp_auth_ix],
        Some(&payer.pubkey()),
        &[&payer, &mint_authority],
        svm.latest_blockhash(),
    );
    println!("Sending setup transaction for vaults and LP authority...");
    map_litesvm_err(svm.send_transaction(setup_tx))?;
    println!("Setup transaction successful.");

    // 8. Initialize the Pool
    let init_ix = Instruction {
        program_id: dex_pid,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),             // 0 Payer (Signer)
            AccountMeta::new(pool_pda, false),                  // 1 Pool State (Writable, NOT Signer)
            AccountMeta::new(vault_a_pk, false),                // 2 Vault A (Writable, NOT Signer)
            AccountMeta::new(vault_b_pk, false),                // 3 Vault B (Writable, NOT Signer)
            AccountMeta::new(lp_mint, false),                   // 4 LP Mint (Writable, NOT Signer)
            AccountMeta::new_readonly(mint_a, false),           // 5 Mint A (Readonly)
            AccountMeta::new_readonly(mint_b, false),           // 6 Mint B (Readonly)
            AccountMeta::new_readonly(plugin_pid, false),       // 7 Plugin Program (Readonly)
            AccountMeta::new(plugin_state_pk, false),           // 8 Plugin State (Writable, NOT Signer)
            AccountMeta::new_readonly(system_program::id(), false), // 9 System Program (Readonly)
            AccountMeta::new_readonly(sysvar::rent::id(), false),   // 10 Rent Sysvar (Readonly)
            AccountMeta::new_readonly(spl_token::id(), false),      // 11 Token Program (Readonly)
        ],
        data: PoolInstruction::InitializePool.try_to_vec()?,
    };

    // --- Diagnostic Logging for InitializePool TX ---
    println!("InitializePool IX Accounts: {:?}", init_ix.accounts.iter().map(|am| (am.pubkey, am.is_signer, am.is_writable)).collect::<Vec<_>>());
    println!("InitializePool IX Data Len: {}", init_ix.data.len());

    // Use manual message construction
    let latest_blockhash = svm.latest_blockhash();
    let message = Message::new(&[init_ix], Some(&payer.pubkey()));
    println!("Constructed Message: {:#?}", message);
    println!("Message Account Keys: {:?}", message.account_keys);
    println!("Message Header (num_required_signatures, num_readonly_signed, num_readonly_unsigned): ({}, {}, {})", message.header.num_required_signatures, message.header.num_readonly_signed_accounts, message.header.num_readonly_unsigned_accounts);

    let mut tx = Transaction::new_unsigned(message); // Create unsigned
    println!("Attempting to sign Tx with Payer: {}", payer.pubkey());
    println!("Blockhash for signing: {}", latest_blockhash);
    tx.sign(&[&payer], latest_blockhash); // Sign
    println!("Transaction signed successfully (apparently).");

    println!("Sending InitializePool transaction...");
    map_litesvm_err(svm.send_transaction(tx))?;
    println!("Pool Initialization successful during setup.");

    Ok(TestSetup {
        svm,
        payer,
        mint_authority,
        dex_pid,
        plugin_pid,
        mint_a,
        mint_b,
        lp_mint,
        plugin_state_pk,
        pool_pda,
        pool_bump,
        vault_a_pk,
        vault_b_pk,
    })
}

// Test Pool Initialization (using the setup function)
#[test]
fn test_initialize_pool_litesvm() -> Result<(), Box<dyn std::error::Error>> {
    let setup = setup_test_environment()?;
    // Use helper to get pool state
    let pool_state = get_pool_state(&setup.svm, &setup.pool_pda)?;
    // Assertions remain mostly the same, just using the fetched state
    assert_eq!(pool_state.token_mint_a, setup.mint_a, "Mint A mismatch");
    assert_eq!(pool_state.token_mint_b, setup.mint_b, "Mint B mismatch");
    assert_eq!(pool_state.vault_a, setup.vault_a_pk, "Vault A mismatch");
    assert_eq!(pool_state.vault_b, setup.vault_b_pk, "Vault B mismatch");
    assert_eq!(pool_state.lp_mint, setup.lp_mint, "LP Mint mismatch");
    assert_eq!(pool_state.total_lp_supply, 0, "Initial LP supply non-zero");
    assert_eq!(pool_state.bump, setup.pool_bump, "Bump mismatch");
    assert_eq!(
        pool_state.plugin_program_id, setup.plugin_pid,
        "Plugin PID mismatch"
    );
    assert_eq!(
        pool_state.plugin_state_pubkey, setup.plugin_state_pk,
        "Plugin State mismatch"
    );
    println!("Pool State Assertions Passed!");
    Ok(())
}

// Test Add Liquidity
#[test]
fn test_add_liquidity_simple() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // Setup user
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;

    // Mint initial tokens
    let initial_a_amount = 1_234_567;
    let initial_b_amount = 7_654_321;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &user_ata_a,
        initial_a_amount,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        initial_b_amount,
    )?;
    assert_eq!(get_token_balance(&setup.svm, &user_ata_a), initial_a_amount);
    assert_eq!(get_token_balance(&setup.svm, &user_ata_b), initial_b_amount);

    // Add liquidity using helper
    let deposit_a = 123_456;
    let deposit_b = 654_321;
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a,
        deposit_b,
    )?;

    // Assert balances
    assert_eq!(
        get_token_balance(&setup.svm, &user_ata_a),
        initial_a_amount - deposit_a
    );
    assert_eq!(
        get_token_balance(&setup.svm, &user_ata_b),
        initial_b_amount - deposit_b
    );
    assert_eq!(get_token_balance(&setup.svm, &setup.vault_a_pk), deposit_a);
    assert_eq!(get_token_balance(&setup.svm, &setup.vault_b_pk), deposit_b);
    let user_lp_balance = get_token_balance(&setup.svm, &user_ata_lp);
    println!("User received {} LP tokens", user_lp_balance);
    assert!(user_lp_balance > 0, "User should receive LP tokens");

    // Use helper to get pool state
    let pool_state = get_pool_state(&setup.svm, &setup.pool_pda)?;
    assert_eq!(
        pool_state.total_lp_supply, user_lp_balance,
        "Pool total LP supply mismatch"
    );

    println!("Add Liquidity Test Passed!");
    Ok(())
}

// Test Remove Liquidity
#[test]
fn test_remove_liquidity_simple() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // Setup user and initial liquidity
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let deposit_a = 123_456;
    let deposit_b = 654_321;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &user_ata_a,
        deposit_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        deposit_b,
    )?;
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a,
        deposit_b,
    )?;

    let initial_lp_balance = get_token_balance(&setup.svm, &user_ata_lp);
    let initial_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let initial_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);
    let initial_user_a = get_token_balance(&setup.svm, &user_ata_a);
    let initial_user_b = get_token_balance(&setup.svm, &user_ata_b);
    assert!(initial_lp_balance > 0);

    // Remove all LP
    let remove_amount_lp = initial_lp_balance;
    let remove_liq_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(user_kp.pubkey(), true),  // 0 user signer
            AccountMeta::new(setup.pool_pda, false),   // 1 pool state
            AccountMeta::new(setup.vault_a_pk, false), // 2 vault a
            AccountMeta::new(setup.vault_b_pk, false), // 3 vault b
            AccountMeta::new(setup.lp_mint, false),    // 4 lp mint
            AccountMeta::new(user_ata_a, false),       // 5 user token A
            AccountMeta::new(user_ata_b, false),       // 6 user token B
            AccountMeta::new(user_ata_lp, false),      // 7 user LP
            AccountMeta::new_readonly(spl_token::id(), false), // 8 token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // 9 plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // 10 plugin state
        ],
        data: PoolInstruction::RemoveLiquidity {
            amount_lp: remove_amount_lp,
        }
        .try_to_vec()?,
    };

    let remove_tx = Transaction::new_signed_with_payer(
        &[remove_liq_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, &user_kp], // Payer + User must sign
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(remove_tx))?;

    // Assert balances
    assert_eq!(get_token_balance(&setup.svm, &user_ata_lp), 0);
    assert_eq!(get_token_balance(&setup.svm, &setup.vault_a_pk), 0);
    assert_eq!(get_token_balance(&setup.svm, &setup.vault_b_pk), 0);
    let final_user_a = get_token_balance(&setup.svm, &user_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &user_ata_b);
    println!(
        "User received A: {}, B: {}",
        final_user_a - initial_user_a,
        final_user_b - initial_user_b
    );
    assert_eq!(final_user_a, initial_user_a + initial_vault_a);
    assert_eq!(final_user_b, initial_user_b + initial_vault_b);

    let pool_state = get_pool_state(&setup.svm, &setup.pool_pda)?;
    assert_eq!(pool_state.total_lp_supply, 0);

    println!("Remove Liquidity Test Passed!");
    Ok(())
}

// Test Partial Remove Liquidity
#[test]
fn test_remove_liquidity_partial() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // Setup user and initial liquidity (simple amounts)
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let deposit_a = 100;
    let deposit_b = 200;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &user_ata_a,
        deposit_a * 2,
    )?; // Mint more than needed
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        deposit_b * 2,
    )?;
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a,
        deposit_b,
    )?;

    let lp_balance_after_add = get_token_balance(&setup.svm, &user_ata_lp);
    let vault_a_after_add = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let vault_b_after_add = get_token_balance(&setup.svm, &setup.vault_b_pk);
    let user_a_after_add = get_token_balance(&setup.svm, &user_ata_a);
    let user_b_after_add = get_token_balance(&setup.svm, &user_ata_b);
    let pool_state_after_add = get_pool_state(&setup.svm, &setup.pool_pda)?;
    let total_lp_after_add = pool_state_after_add.total_lp_supply;
    println!(
        "LP Balance after add: {}, Vaults: A={}, B={}",
        lp_balance_after_add, vault_a_after_add, vault_b_after_add
    );

    // Remove amount chosen to force rounding
    let remove_amount_lp = 50;
    assert!(lp_balance_after_add >= remove_amount_lp);
    let remove_liq_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(user_kp.pubkey(), true),  // 0 user signer
            AccountMeta::new(setup.pool_pda, false),   // 1 pool state
            AccountMeta::new(setup.vault_a_pk, false), // 2 vault a
            AccountMeta::new(setup.vault_b_pk, false), // 3 vault b
            AccountMeta::new(setup.lp_mint, false),    // 4 lp mint
            AccountMeta::new(user_ata_a, false),       // 5 user token A
            AccountMeta::new(user_ata_b, false),       // 6 user token B
            AccountMeta::new(user_ata_lp, false),      // 7 user LP
            AccountMeta::new_readonly(spl_token::id(), false), // 8 token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // 9 plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // 10 plugin state
        ],
        data: PoolInstruction::RemoveLiquidity {
            amount_lp: remove_amount_lp,
        }
        .try_to_vec()?,
    };

    let remove_tx = Transaction::new_signed_with_payer(
        &[remove_liq_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, &user_kp], // Payer + User must sign
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(remove_tx))?;

    // Assert balances
    let final_user_lp = get_token_balance(&setup.svm, &user_ata_lp);
    let final_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let final_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);
    let final_user_a = get_token_balance(&setup.svm, &user_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &user_ata_b);
    assert_eq!(final_user_lp, lp_balance_after_add - remove_amount_lp);
    let received_a = final_user_a - user_a_after_add;
    let received_b = final_user_b - user_b_after_add;

    // Calculate theoretical integer amounts using u128 for precision
    let theoretical_a_num = (vault_a_after_add as u128) * (remove_amount_lp as u128);
    let theoretical_a_out_floor = theoretical_a_num / (total_lp_after_add as u128);
    let remainder_a = theoretical_a_num % (total_lp_after_add as u128);

    let theoretical_b_num = (vault_b_after_add as u128) * (remove_amount_lp as u128);
    let theoretical_b_out_floor = theoretical_b_num / (total_lp_after_add as u128);
    let remainder_b = theoretical_b_num % (total_lp_after_add as u128);

    // Calculate ceiling amounts based on spl-math logic (add 1 if remainder > 0)
    let theoretical_a_out_ceil = theoretical_a_out_floor + if remainder_a != 0 { 1 } else { 0 };
    let theoretical_b_out_ceil = theoretical_b_out_floor + if remainder_b != 0 { 1 } else { 0 };

    // Calculate theoretical float amounts (for logging comparison)
    let theoretical_a_out_float =
        (vault_a_after_add as f64) * (remove_amount_lp as f64) / (total_lp_after_add as f64);
    let theoretical_b_out_float =
        (vault_b_after_add as f64) * (remove_amount_lp as f64) / (total_lp_after_add as f64);

    println!(
        "Theoretical (float) out A: {:.6}, B: {:.6}",
        theoretical_a_out_float, theoretical_b_out_float
    );
    println!(
        "Theoretical (floor) out A: {}, B: {}",
        theoretical_a_out_floor, theoretical_b_out_floor
    );
    println!(
        "Theoretical (ceil) out A: {}, B: {}",
        theoretical_a_out_ceil, theoretical_b_out_ceil
    );
    println!("Actual received A: {}, B: {}", received_a, received_b);
    // Log the difference vs ceiling (shows how close the actual is to the fair ceil amount)
    println!(
        "User shortfall vs ceil A: {:.6}, B: {:.6}",
        theoretical_a_out_float - (received_a as f64),
        theoretical_b_out_float - (received_b as f64)
    );
    // Log the amount kept by pool if simple floor were used
    println!(
        "Amount kept by pool if floor used A: {:.6}, B: {:.6}",
        theoretical_a_out_float - (theoretical_a_out_floor as f64),
        theoretical_b_out_float - (theoretical_b_out_floor as f64)
    );
    println!(
        "Vaults remaining A: {}, B: {}",
        final_vault_a, final_vault_b
    );

    // Assert user received EXACTLY the floored amount (since plugin now uses floor)
    assert_eq!(
        received_a, theoretical_a_out_floor as u64,
        "Rounding for A was not floor"
    );
    assert_eq!(
        received_b, theoretical_b_out_floor as u64,
        "Rounding for B was not floor"
    );

    let pool_state_final = get_pool_state(&setup.svm, &setup.pool_pda)?;
    assert_eq!(
        pool_state_final.total_lp_supply,
        total_lp_after_add - remove_amount_lp
    );

    println!("Partial Remove Liquidity Test Passed!");
    Ok(())
}

// Test Swap (A -> B)
#[test]
fn test_swap_a_to_b() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;

    // --- Initial Liquidity Setup (using setup.payer) ---
    let deposit_a = 123_456;
    let deposit_b = 654_321;
    let payer_ata_a = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_a,
    )?;
    let payer_ata_b = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_b,
    )?;
    let payer_ata_lp = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.lp_mint,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &payer_ata_a,
        deposit_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &payer_ata_b,
        deposit_b,
    )?;
    // Clone payer keypair to pass as the depositor identity
    let payer_kp_clone =
        Keypair::from_bytes(&setup.payer.to_bytes()).expect("Failed to clone payer keypair");
    execute_add_liquidity(
        &mut setup,
        &payer_kp_clone,
        &payer_ata_a,
        &payer_ata_b,
        &payer_ata_lp,
        deposit_a,
        deposit_b,
    )?;
    // --- End Initial Liquidity ---

    // Setup Swapper User
    let (swapper_kp, swapper_ata_a, swapper_ata_b, _swapper_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let initial_swapper_a = 1_234_567;
    let initial_swapper_b = 7_654_321;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &swapper_ata_a,
        initial_swapper_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &swapper_ata_b,
        initial_swapper_b,
    )?;

    let initial_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let initial_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let initial_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let initial_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Build Swap IX (A -> B)
    let amount_in = 11_111;
    let min_out = 1;
    let effective_in = (amount_in as u128) * 997 / 1000;
    let swap_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(swapper_kp.pubkey(), true), // User is the swapper
            AccountMeta::new(setup.pool_pda, false),
            AccountMeta::new(setup.vault_a_pk, false),
            AccountMeta::new(setup.vault_b_pk, false),
            AccountMeta::new(swapper_ata_a, false), // Swapper's source ATA A
            AccountMeta::new(swapper_ata_b, false), // Swapper's destination ATA B
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(setup.plugin_pid, false),
            AccountMeta::new(setup.plugin_state_pk, false),
        ],
        data: PoolInstruction::Swap { amount_in, min_out }.try_to_vec()?,
    };

    // Send SWAP TX
    let swap_tx = Transaction::new_signed_with_payer(
        &[swap_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, &swapper_kp],
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(swap_tx))?;

    // Assert balances
    let final_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let final_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let final_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Calculate expected output (simple CPMM with 0.3% fee)
    // amount_out = (reserve_b * effective_in) / (reserve_a + effective_in)
    let expected_amount_out =
        (initial_vault_b as u128 * effective_in) / (initial_vault_a as u128 + effective_in);

    println!("--- Swap A to B ---");
    println!("Initial User A: {}", initial_user_a);
    println!("Initial User B: {}", initial_user_b);
    println!("Initial Vault A: {}", initial_vault_a);
    println!("Initial Vault B: {}", initial_vault_b);
    println!("Amount In (A): {}", amount_in);
    println!("Effective In (A): {}", effective_in);
    println!("Expected Amount Out (B): {}", expected_amount_out);
    println!("Final User A: {}", final_user_a);
    println!("Final User B: {}", final_user_b);
    println!("Final Vault A: {}", final_vault_a);
    println!("Final Vault B: {}", final_vault_b);

    // Assertions
    assert_eq!(
        final_user_a,
        initial_user_a - amount_in,
        "User A balance mismatch"
    );
    assert_eq!(
        final_user_b,
        initial_user_b + expected_amount_out as u64, // Cast u128 to u64
        "User B balance mismatch"
    );
    assert_eq!(
        final_vault_a,
        initial_vault_a + amount_in,
        "Vault A balance mismatch"
    );
    assert_eq!(
        final_vault_b,
        initial_vault_b - expected_amount_out as u64, // Cast u128 to u64
        "Vault B balance mismatch"
    );
    // Check minimum out constraint
    assert!(
        expected_amount_out >= min_out as u128,
        "Swap output less than minimum required"
    );

    println!("Swap A->B Test Passed!");
    Ok(())
}

// Test Swap (B -> A)
#[test]
fn test_swap_b_to_a() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // --- Initial Liquidity Setup (using setup.payer) ---
    let deposit_a = 123_456;
    let deposit_b = 654_321;
    let payer_ata_a = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_a,
    )?;
    let payer_ata_b = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_b,
    )?;
    let payer_ata_lp = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.lp_mint,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &payer_ata_a,
        deposit_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &payer_ata_b,
        deposit_b,
    )?;
    // Clone payer keypair to pass as the depositor identity
    let payer_kp_clone =
        Keypair::from_bytes(&setup.payer.to_bytes()).expect("Failed to clone payer keypair");
    execute_add_liquidity(
        &mut setup,
        &payer_kp_clone,
        &payer_ata_a,
        &payer_ata_b,
        &payer_ata_lp,
        deposit_a,
        deposit_b,
    )?;
    // --- End Initial Liquidity ---

    // Setup Swapper User
    let (swapper_kp, swapper_ata_a, swapper_ata_b, _swapper_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let initial_swapper_a = 1_234_567;
    let initial_swapper_b = 7_654_321;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &swapper_ata_a,
        initial_swapper_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &swapper_ata_b,
        initial_swapper_b,
    )?;

    let initial_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let initial_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let initial_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let initial_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Build Swap IX (B -> A)
    let amount_in = 22_222;
    let min_out = 1;
    let effective_in = (amount_in as u128) * 997 / 1000;
    let swap_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(swapper_kp.pubkey(), true), // 0 user signer
            AccountMeta::new_readonly(setup.pool_pda, false), // 1 pool state (readonly for swap ix itself)
            AccountMeta::new(setup.vault_a_pk, false), // 2 vault a (sending A) - Mark Writable
            AccountMeta::new(setup.vault_b_pk, false), // 3 vault b (receiving B) - Mark Writable
            AccountMeta::new(swapper_ata_b, false),    // 4 user src token (B) - Mark Writable
            AccountMeta::new(swapper_ata_a, false),    // 5 user dst token (A) - Mark Writable
            AccountMeta::new_readonly(spl_token::id(), false), // 6 token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // 7 plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // 8 plugin state - Mark Writable
        ],
        data: PoolInstruction::Swap { amount_in, min_out }.try_to_vec()?,
    };

    // Pre-check and Send SWAP TX
    let swap_tx = Transaction::new_signed_with_payer(
        &[swap_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, &swapper_kp],
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(swap_tx))?;

    // Assert balances
    let final_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let final_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let final_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Calculate expected output (simple CPMM with 0.3% fee)
    // amount_out_a = (reserve_a * effective_in_b) / (reserve_b + effective_in_b)
    let expected_amount_out =
        (initial_vault_a as u128 * effective_in) / (initial_vault_b as u128 + effective_in);

    println!("--- Swap B to A ---");
    println!("Initial User A: {}", initial_user_a);
    println!("Initial User B: {}", initial_user_b);
    println!("Initial Vault A: {}", initial_vault_a);
    println!("Initial Vault B: {}", initial_vault_b);
    println!("Amount In (B): {}", amount_in);
    println!("Effective In (B): {}", effective_in);
    println!("Expected Amount Out (A): {}", expected_amount_out);
    println!("Final User A: {}", final_user_a);
    println!("Final User B: {}", final_user_b);
    println!("Final Vault A: {}", final_vault_a);
    println!("Final Vault B: {}", final_vault_b);

    // Assertions
    assert_eq!(
        final_user_b,
        initial_user_b - amount_in,
        "User B balance mismatch"
    );
    assert_eq!(
        final_user_a,
        initial_user_a + expected_amount_out as u64, // Cast u128 to u64
        "User A balance mismatch"
    );
    assert_eq!(
        final_vault_b,
        initial_vault_b + amount_in,
        "Vault B balance mismatch"
    );
    assert_eq!(
        final_vault_a,
        initial_vault_a - expected_amount_out as u64, // Cast u128 to u64
        "Vault A balance mismatch"
    );
    // Check minimum out constraint
    assert!(
        expected_amount_out >= min_out as u128,
        "Swap output less than minimum required"
    );

    println!("Swap B->A Test Passed!");
    Ok(())
}

// Test Add Liquidity with Refund (depositing excess A)
#[test]
fn test_add_liquidity_refund() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // --- Initial deposit (using setup.payer) ---
    let initial_deposit_a = 100_000;
    let initial_deposit_b = 500_000;
    let payer_ata_a = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_a,
    )?;
    let payer_ata_b = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.mint_b,
    )?;
    let payer_ata_lp = create_user_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.payer.pubkey(),
        &setup.lp_mint,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &payer_ata_a,
        initial_deposit_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &payer_ata_b,
        initial_deposit_b,
    )?;
    // Clone payer keypair to pass as the depositor identity
    let payer_kp_clone =
        Keypair::from_bytes(&setup.payer.to_bytes()).expect("Failed to clone payer keypair");
    execute_add_liquidity(
        &mut setup,
        &payer_kp_clone,
        &payer_ata_a,
        &payer_ata_b,
        &payer_ata_lp,
        initial_deposit_a,
        initial_deposit_b,
    )?;
    // --- End Initial Deposit ---

    // --- Second deposit (user tries to add too much A) ---
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let user_initial_a = 500_000;
    let user_initial_b = 500_000;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &user_ata_a,
        user_initial_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        user_initial_b,
    )?;

    let user_balance_a_before = get_token_balance(&setup.svm, &user_ata_a);
    let user_balance_b_before = get_token_balance(&setup.svm, &user_ata_b);
    let vault_a_before = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let vault_b_before = get_token_balance(&setup.svm, &setup.vault_b_pk);
    let pool_state_before = get_pool_state(&setup.svm, &setup.pool_pda)?;
    let total_lp_before = pool_state_before.total_lp_supply;
    let user_lp_before = get_token_balance(&setup.svm, &user_ata_lp);

    // User tries to deposit 30k A (excess) and 50k B.
    let deposit_a_attempt = 30_000;
    let deposit_b_attempt = 50_000;
    // Use helper to execute the add liquidity
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a_attempt,
        deposit_b_attempt,
    )?;

    // --- Assertions ---
    let user_balance_a_after = get_token_balance(&setup.svm, &user_ata_a);
    let user_balance_b_after = get_token_balance(&setup.svm, &user_ata_b);
    let vault_a_after = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let vault_b_after = get_token_balance(&setup.svm, &setup.vault_b_pk);
    let user_lp_after = get_token_balance(&setup.svm, &user_ata_lp);
    let pool_state_after = get_pool_state(&setup.svm, &setup.pool_pda)?;
    let total_lp_after = pool_state_after.total_lp_supply;

    // Calculate expected amounts based on pool ratio and deposit_b_attempt
    // actual_deposit_a = floor(vault_a_before * deposit_b_attempt / vault_b_before)
    let actual_deposit_a =
        (vault_a_before as u128 * deposit_b_attempt as u128 / vault_b_before as u128) as u64;
    let actual_deposit_b = deposit_b_attempt; // B is the limiting factor
    let refunded_a = deposit_a_attempt - actual_deposit_a;

    // Calculate expected LP minted based on existing pool ratio (using A ratio here)
    // Since initial liquidity was added, vaults and total_lp are non-zero.
    // lp_minted = floor(actual_deposit_a * total_lp_before / vault_a_before)
    let expected_lp_minted =
        (actual_deposit_a as u128 * total_lp_before as u128 / vault_a_before as u128) as u64;

    println!("--- Add Liquidity Refund ---");
    println!("Vaults Before: A={}, B={}", vault_a_before, vault_b_before);
    println!(
        "User Before: A={}, B={}, LP={}",
        user_balance_a_before, user_balance_b_before, user_lp_before
    );
    println!(
        "Attempted Deposit: A={}, B={}",
        deposit_a_attempt, deposit_b_attempt
    );
    println!(
        "Actual Deposit: A={}, B={}",
        actual_deposit_a, actual_deposit_b
    );
    println!("Refunded A: {}", refunded_a);
    println!("Expected LP Minted: {}", expected_lp_minted);
    println!("Vaults After: A={}, B={}", vault_a_after, vault_b_after);
    println!(
        "User After: A={}, B={}, LP={}",
        user_balance_a_after, user_balance_b_after, user_lp_after
    );
    println!(
        "Total LP Before: {}, After: {}",
        total_lp_before, total_lp_after
    );

    // Assert user balances
    assert_eq!(
        user_balance_a_after,
        user_balance_a_before - actual_deposit_a,
        "User A balance mismatch"
    );
    assert_eq!(
        user_balance_b_after,
        user_balance_b_before - actual_deposit_b,
        "User B balance mismatch"
    );
    assert_eq!(
        user_lp_after,
        user_lp_before + expected_lp_minted,
        "User LP balance mismatch"
    );

    // Assert vault balances
    assert_eq!(
        vault_a_after,
        vault_a_before + actual_deposit_a,
        "Vault A balance mismatch"
    );
    assert_eq!(
        vault_b_after,
        vault_b_before + actual_deposit_b,
        "Vault B balance mismatch"
    );

    // Assert total LP supply
    assert_eq!(
        total_lp_after,
        total_lp_before + expected_lp_minted,
        "Total LP supply mismatch"
    );

    println!("Add Liquidity Refund Test Passed!");
    Ok(())
}

// Test Add Liquidity with zero amount A (should fail)
#[test]
fn test_add_liquidity_zero_a() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let initial_b_amount = 500_000;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        initial_b_amount,
    )?;

    // Add liquidity using helper, expect error
    let deposit_a = 0;
    let deposit_b = 50_000;
    let result = execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a,
        deposit_b,
    );

    println!("Add Liquidity Zero A Result: {:?}", result);
    assert!(
        result.is_err(),
        "Transaction should fail with zero amount A"
    );
    Ok(())
}

// Test Initialize Pool when already initialized (should fail)
#[test]
fn test_initialize_pool_already_exists() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    setup.svm.warp_to_slot(100); // Advance clock slightly just in case

    // Rebuild the init instruction locally
    let init_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(setup.payer.pubkey(), true),
            AccountMeta::new(setup.pool_pda, false),             // Writable=false OK
            AccountMeta::new(setup.vault_a_pk, false),             // Writable=false OK
            AccountMeta::new(setup.vault_b_pk, false),             // Writable=false OK
            AccountMeta::new(setup.lp_mint, false),                // Writable=false OK
            AccountMeta::new_readonly(setup.mint_a, false),
            AccountMeta::new_readonly(setup.mint_b, false),
            AccountMeta::new_readonly(setup.plugin_pid, false),
            AccountMeta::new(setup.plugin_state_pk, false),        // Writable=false OK
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false), // ADDED
        ],
        data: PoolInstruction::InitializePool.try_to_vec()?,
    };

    // Add a unique memo instruction to make the TX signature different
    let memo_ix = spl_memo::build_memo(
        format!("Re-init attempt for {}", setup.payer.pubkey()).as_bytes(), // Simpler memo content
        &[&setup.payer.pubkey()], // Payer is signer for memo too
    );

    // Use manual message construction with BOTH instructions
    let latest_blockhash = setup.svm.latest_blockhash();
    let message = Message::new(&[init_ix, memo_ix], Some(&setup.payer.pubkey())); // Include memo_ix
    println!("Constructed Re-Init Message: {:#?}", message);
    println!("Re-Init Message Account Keys: {:?}", message.account_keys);
    println!("Re-Init Message Header: ({}, {}, {})", message.header.num_required_signatures, message.header.num_readonly_signed_accounts, message.header.num_readonly_unsigned_accounts);

    let mut tx = Transaction::new_unsigned(message); // Create unsigned
    println!("Attempting to sign Re-Init Tx with Payer: {}", setup.payer.pubkey());
    println!("Blockhash for Re-Init signing: {}", latest_blockhash);
    tx.sign(&[&setup.payer], latest_blockhash); // Sign
    println!("Re-Init Transaction signed successfully (apparently).");

    println!("Sending Re-InitializePool transaction...");
    let result: Result<TransactionMetadata, FailedTransactionMetadata> = setup.svm.send_transaction(tx);
    assert!(result.is_err(), "Re-initialize TX should fail");

    // Extract the FailedTransactionMetadata since we asserted it's Err
    let failed_metadata = result.err().expect("Assertion failed: result was not Err");

    // Access the TransactionError via the correct `.err` field
    let tx_error = failed_metadata.err;

    // Log only the extracted error to avoid borrow issues
    println!("Re-Initialize Pool Error: {:?}", tx_error);

    // Assert the error is an InstructionError at index 0 (our InitializePool instruction)
    match tx_error {
        solana_sdk::transaction::TransactionError::InstructionError(0, ref instruction_error) => {
            // We expect the system_instruction::create_account CPI inside InitializePool to fail
            // because the account (pool_pda) already exists.
            // This often manifests as InstructionError::Custom(0) if not mapped,
            // or potentially AccountAlreadyInitialized or others depending on exact runtime/SDK.
            // Let's check if it's *some* instruction error, which is sufficient for this test.
            println!(
                "Successfully caught expected InstructionError at index 0: {:?}",
                instruction_error
            );
        }
        _ => panic!(
            "Expected InstructionError at index 0 for second initialization attempt, but got: {:?}",
            tx_error
        ),
    }

    Ok(())
}

// Test Remove Liquidity with zero amount (should fail)
#[test]
fn test_remove_liquidity_zero() -> Result<(), Box<dyn Error>> {
    let mut setup = setup_test_environment()?;
    // Add initial liquidity
    let (user_kp, user_ata_a, user_ata_b, user_ata_lp) = setup_user_accounts(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_a,
        &setup.mint_b,
        &setup.lp_mint,
    )?;
    let deposit_a = 100_000;
    let deposit_b = 500_000;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_a,
        &user_ata_a,
        deposit_a,
    )?;
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        deposit_b,
    )?;
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_ata_a,
        &user_ata_b,
        &user_ata_lp,
        deposit_a,
        deposit_b,
    )?;
    assert!(get_token_balance(&setup.svm, &user_ata_lp) > 0);

    // Attempt to remove 0 LP
    let remove_amount_lp = 0;
    let remove_liq_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(user_kp.pubkey(), true),  // 0 user signer
            AccountMeta::new(setup.pool_pda, false),   // 1 pool state
            AccountMeta::new(setup.vault_a_pk, false), // 2 vault a
            AccountMeta::new(setup.vault_b_pk, false), // 3 vault b
            AccountMeta::new(setup.lp_mint, false),    // 4 lp mint
            AccountMeta::new(user_ata_a, false),       // 5 user token A
            AccountMeta::new(user_ata_b, false),       // 6 user token B
            AccountMeta::new(user_ata_lp, false),      // 7 user LP
            AccountMeta::new_readonly(spl_token::id(), false), // 8 token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // 9 plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // 10 plugin state
        ],
        data: PoolInstruction::RemoveLiquidity {
            amount_lp: remove_amount_lp,
        }
        .try_to_vec()?,
    };

    let remove_tx = Transaction::new_signed_with_payer(
        &[remove_liq_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, &user_kp],
        setup.svm.latest_blockhash(),
    );
    let result = setup.svm.send_transaction(remove_tx);
    assert!(
        result.is_err(),
        "Transaction should fail when removing zero LP"
    );
    Ok(())
}
