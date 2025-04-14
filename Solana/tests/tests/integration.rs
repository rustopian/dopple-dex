use {
    borsh::{BorshDeserialize, BorshSerialize},
    dex_pool_program::instruction::PoolInstruction,
    dex_pool_program::processor::PluginCalcResult,
    dex_pool_program::state::PoolState,
    litesvm::{
        types::{FailedTransactionMetadata, TransactionMetadata},
        LiteSVM,
    },
    solana_program::{program_option::COption, system_instruction},
    solana_sdk::{
        account::Account,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_token::LAMPORTS_PER_SOL,
        pubkey::Pubkey,
        signature::Signer,
        signer::keypair::Keypair,
        system_program,
        sysvar::{self, rent::Rent},
        transaction::Transaction,
    },
    spl_associated_token_account::{
        self, get_associated_token_address, instruction::create_associated_token_account,
    },
    spl_token::{self, solana_program::program_pack::Pack},
    std::env,
    std::error::Error,
    std::mem::size_of,
};

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

// Helper function to handle litesvm errors
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
    let rent = svm.get_sysvar::<Rent>();
    let mint_rent = rent.minimum_balance(spl_token::state::Mint::LEN);

    let create_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &mint_pk,
        mint_rent,
        spl_token::state::Mint::LEN as u64,
        &spl_token::id(),
    );

    let init_ix = spl_token::instruction::initialize_mint(
        &spl_token::id(),
        &mint_pk,
        mint_authority,
        None,
        0,
    )?;

    let tx = Transaction::new_signed_with_payer(
        &[create_ix, init_ix],
        Some(&payer.pubkey()),
        &[payer, &mint_kp],
        svm.latest_blockhash(),
    );

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
        &[payer, mint_authority],
        svm.latest_blockhash(),
    );
    map_litesvm_err(svm.send_transaction(tx))?;
    Ok(())
}

fn get_token_balance(svm: &LiteSVM, ata_pk: &Pubkey) -> u64 {
    svm.get_account(ata_pk)
        .map(|acc| spl_token::state::Account::unpack(&acc.data).unwrap().amount)
        .unwrap_or(0)
}

fn wrap_sol(
    svm: &mut LiteSVM,
    payer: &Keypair,
    user_kp: &Keypair,
    amount: u64,
) -> Result<Pubkey, Box<dyn Error>> {
    let user_pk = user_kp.pubkey();
    let wsol_mint = spl_token::native_mint::id();
    let user_wsol_ata = get_associated_token_address(&user_pk, &wsol_mint);

    let rent = svm.get_sysvar::<Rent>();
    let ata_rent = rent.minimum_balance(spl_token::state::Account::LEN);

    let create_ata_ix =
        create_associated_token_account(&payer.pubkey(), &user_pk, &wsol_mint, &spl_token::id());

    let transfer_ix = system_instruction::transfer(&user_pk, &user_wsol_ata, amount);

    let sync_native_ix = spl_token::instruction::sync_native(&spl_token::id(), &user_wsol_ata)?;

    let tx = Transaction::new_signed_with_payer(
        &[create_ata_ix, transfer_ix, sync_native_ix],
        Some(&payer.pubkey()),
        &[payer, user_kp],
        svm.latest_blockhash(),
    );

    let user_sol_before = svm.get_balance(&user_pk).unwrap_or(0);
    println!(
        "Wrapping SOL: User {} SOL balance before: {}",
        user_pk, user_sol_before
    );

    let send_result = svm.send_transaction(tx);

    let user_sol_after = svm.get_balance(&user_pk).unwrap_or(0);
    let wsol_ata_balance = get_token_balance(svm, &user_wsol_ata);
    let wsol_ata_lamports = svm.get_balance(&user_wsol_ata).unwrap_or(0);
    println!(
        "Wrapping SOL: User {} SOL balance after: {} (change: {})",
        user_pk,
        user_sol_after,
        user_sol_before as i64 - user_sol_after as i64
    );
    println!(
        "Wrapping SOL: wSOL ATA {} token balance: {}",
        user_wsol_ata, wsol_ata_balance
    );
    println!(
        "Wrapping SOL: wSOL ATA {} lamport balance: {}",
        user_wsol_ata, wsol_ata_lamports
    );

    map_litesvm_err(send_result)?;

    assert_eq!(
        get_token_balance(svm, &user_wsol_ata),
        amount,
        "wSOL ATA balance should match wrapped amount"
    );
    assert!(
        wsol_ata_lamports >= amount + ata_rent,
        "wSOL ATA lamports mismatch"
    );

    Ok(user_wsol_ata)
}

fn unwrap_wsol(
    svm: &mut LiteSVM,
    payer: &Keypair,
    user_kp: &Keypair,
    user_wsol_ata: &Pubkey,
) -> Result<(), Box<dyn Error>> {
    let user_pk = user_kp.pubkey();

    let user_sol_before = svm.get_balance(&user_pk).unwrap_or(0);
    let wsol_balance_before = get_token_balance(svm, user_wsol_ata);
    let wsol_lamports_before = svm.get_balance(user_wsol_ata).unwrap_or(0);
    println!(
        "Unwrapping wSOL: User {} SOL balance before: {}",
        user_pk, user_sol_before
    );
    println!(
        "Unwrapping wSOL: wSOL ATA {} token balance before: {}",
        user_wsol_ata, wsol_balance_before
    );
    println!(
        "Unwrapping wSOL: wSOL ATA {} lamport balance before: {}",
        user_wsol_ata, wsol_lamports_before
    );

    let close_ix = spl_token::instruction::close_account(
        &spl_token::id(),
        user_wsol_ata,
        &user_pk,
        &user_pk,
        &[&user_pk],
    )?;

    let tx = Transaction::new_signed_with_payer(
        &[close_ix],
        Some(&payer.pubkey()),
        &[payer, user_kp],
        svm.latest_blockhash(),
    );

    map_litesvm_err(svm.send_transaction(tx))?;

    let user_sol_after = svm.get_balance(&user_pk).unwrap_or(0);
    let wsol_account_final = svm.get_account(user_wsol_ata);
    println!(
        "Unwrapping wSOL: User {} SOL balance after: {} (change: {})",
        user_pk,
        user_sol_after,
        user_sol_after as i64 - user_sol_before as i64
    );
    println!(
        "Unwrapping wSOL: wSOL ATA {} account after: {:?}",
        user_wsol_ata, wsol_account_final
    );

    assert_eq!(
        wsol_account_final.map(|acc| acc.lamports).unwrap_or(0),
        0,
        "wSOL ATA should be closed (zero lamports)"
    );
    let expected_increase = wsol_lamports_before;
    let actual_increase = user_sol_after - user_sol_before;
    assert!(
        actual_increase >= expected_increase - 5000 && actual_increase <= expected_increase,
        "User SOL balance did not increase correctly after unwrap (expected ~{}, got {})",
        expected_increase,
        actual_increase
    );

    Ok(())
}

fn setup_user_accounts(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint_a: &Pubkey,
    mint_b: &Pubkey,
    mint_lp: &Pubkey,
) -> Result<(Keypair, Pubkey, Pubkey, Pubkey), Box<dyn Error>> {
    let user_kp = Keypair::new();
    let user_pk = user_kp.pubkey();
    map_litesvm_err(svm.airdrop(&user_pk, 1_000_000_000))?;
    let user_ata_a = create_user_ata(svm, payer, &user_pk, mint_a)?;
    let user_ata_b = create_user_ata(svm, payer, &user_pk, mint_b)?;
    let user_ata_lp = create_user_ata(svm, payer, &user_pk, mint_lp)?;
    Ok((user_kp, user_ata_a, user_ata_b, user_ata_lp))
}

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
            AccountMeta::new(user_kp.pubkey(), true),
            AccountMeta::new(setup.pool_pda, false),
            AccountMeta::new(setup.vault_a_pk, false),
            AccountMeta::new(setup.vault_b_pk, false),
            AccountMeta::new(setup.lp_mint, false),
            AccountMeta::new(*user_ata_a, false),
            AccountMeta::new(*user_ata_b, false),
            AccountMeta::new(*user_ata_lp, false),
            AccountMeta::new_readonly(spl_token::id(), false),
            AccountMeta::new_readonly(setup.plugin_pid, false),
            AccountMeta::new(setup.plugin_state_pk, false),
        ],
        data: PoolInstruction::AddLiquidity { amount_a, amount_b }.try_to_vec()?,
    };
    let tx = Transaction::new_signed_with_payer(
        &[add_liq_ix],
        Some(&setup.payer.pubkey()),
        &[&setup.payer, user_kp],
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(tx))?;
    Ok(())
}

fn get_pool_state(svm: &LiteSVM, pool_pda: &Pubkey) -> Result<PoolState, Box<dyn Error>> {
    let pool_account = svm
        .get_account(pool_pda)
        .ok_or_else(|| Box::<dyn Error>::from(format!("Pool account {} not found", pool_pda)))?;
    PoolState::try_from_slice(&pool_account.data)
        .map_err(|e| Box::<dyn Error>::from(format!("Failed to deserialize PoolState: {}", e)))
}

fn setup_test_environment() -> Result<TestSetup, Box<dyn Error>> {
    let dex_pid = Pubkey::new_unique();
    let plugin_pid = Pubkey::new_unique();
    println!("Using DEX Program ID: {}", dex_pid);
    println!("Using Plugin Program ID: {}", plugin_pid);

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

    let mut svm = LiteSVM::new();

    map_litesvm_err(svm.add_program_from_file(dex_pid, &dex_so_path))?;
    map_litesvm_err(svm.add_program_from_file(plugin_pid, &plugin_so_path))?;

    let payer = Keypair::new();
    let mint_authority = Keypair::new();

    map_litesvm_err(svm.airdrop(&payer.pubkey(), 10_000_000_000))?;
    map_litesvm_err(svm.airdrop(&mint_authority.pubkey(), 1_000_000_000))?;

    let mint_a_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;
    let mint_b_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;
    let lp_mint_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;

    let mint_a = mint_a_kp.pubkey();
    let mint_b = mint_b_kp.pubkey();
    let lp_mint = lp_mint_kp.pubkey();

    let (sorted_mint_a, sorted_mint_b) = if mint_a < mint_b {
        (mint_a, mint_b)
    } else {
        (mint_b, mint_a)
    };

    let plugin_state_kp = Keypair::new();
    let plugin_state_pk = plugin_state_kp.pubkey();
    let plugin_state_size = size_of::<PluginCalcResult>();
    println!("Plugin State Account Size: {}", plugin_state_size);
    let rent = svm.get_sysvar::<Rent>();
    let plugin_state_rent = rent.minimum_balance(plugin_state_size);

    let create_plugin_state_ix = solana_sdk::system_instruction::create_account(
        &payer.pubkey(),
        &plugin_state_pk,
        plugin_state_rent,
        plugin_state_size as u64,
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

    let init_ix = Instruction {
        program_id: dex_pid,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(pool_pda, false),
            AccountMeta::new(vault_a_pk, false),
            AccountMeta::new(vault_b_pk, false),
            AccountMeta::new(lp_mint, false),
            AccountMeta::new_readonly(mint_a, false),
            AccountMeta::new_readonly(mint_b, false),
            AccountMeta::new_readonly(plugin_pid, false),
            AccountMeta::new(plugin_state_pk, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: PoolInstruction::InitializePool.try_to_vec()?,
    };

    println!(
        "InitializePool IX Accounts: {:?}",
        init_ix
            .accounts
            .iter()
            .map(|am| (am.pubkey, am.is_signer, am.is_writable))
            .collect::<Vec<_>>()
    );
    println!("InitializePool IX Data Len: {}", init_ix.data.len());

    let latest_blockhash = svm.latest_blockhash();
    let message = Message::new(&[init_ix], Some(&payer.pubkey()));
    println!("Constructed Message: {:#?}", message);
    println!("Message Account Keys: {:?}", message.account_keys);
    println!("Message Header (num_required_signatures, num_readonly_signed, num_readonly_unsigned): ({}, {}, {})", message.header.num_required_signatures, message.header.num_readonly_signed_accounts, message.header.num_readonly_unsigned_accounts);

    let mut tx = Transaction::new_unsigned(message);
    println!("Attempting to sign Tx with Payer: {}", payer.pubkey());
    println!("Blockhash for signing: {}", latest_blockhash);
    tx.sign(&[&payer], latest_blockhash);
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

fn setup_wsol_test_environment() -> Result<TestSetup, Box<dyn Error>> {
    let dex_pid = Pubkey::new_unique();
    let plugin_pid = Pubkey::new_unique();
    println!("Using DEX Program ID: {}", dex_pid);
    println!("Using Plugin Program ID: {}", plugin_pid);

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

    let mut svm = LiteSVM::new();

    // --- Manually add native wSOL mint account to SVM ---
    let native_mint_id = spl_token::native_mint::id();
    let token_program_id = spl_token::id();
    let rent = svm.get_sysvar::<Rent>(); // Get rent sysvar *after* SVM init
    let mint_rent = rent.minimum_balance(spl_token::state::Mint::LEN);

    // Create a default Mint state (COption::None for authority, 9 decimals for SOL)
    let mint_state = spl_token::state::Mint {
        mint_authority: COption::None,
        supply: 0,   // Supply isn't tracked directly for native mint like other SPLs
        decimals: 9, // SOL has 9 decimals
        is_initialized: true,
        freeze_authority: COption::None,
    };
    let mut data_buffer = vec![0u8; spl_token::state::Mint::LEN]; // Allocate buffer
    mint_state.pack_into_slice(&mut data_buffer); // Pack state into buffer

    // Create the Account struct
    let native_mint_account = Account {
        lamports: mint_rent,     // Must have rent
        data: data_buffer,       // Mint state data
        owner: token_program_id, // Critical: Owner must be SPL Token program
        executable: false,
        rent_epoch: 1, // Typically 1 for initialized accounts
    };

    // Add the account to the SVM
    svm.set_account(native_mint_id, native_mint_account);
    println!(
        "Manually added native wSOL mint account {} to SVM with owner {}",
        native_mint_id, token_program_id
    );
    // --- End wSOL mint setup ---

    // 2. Load programs
    map_litesvm_err(svm.add_program_from_file(dex_pid, &dex_so_path))?;
    map_litesvm_err(svm.add_program_from_file(plugin_pid, &plugin_so_path))?;

    let payer = Keypair::new();
    let mint_authority = Keypair::new();

    map_litesvm_err(svm.airdrop(&payer.pubkey(), 10 * LAMPORTS_PER_SOL))?;
    map_litesvm_err(svm.airdrop(&mint_authority.pubkey(), 1 * LAMPORTS_PER_SOL))?;

    let mint_a = spl_token::native_mint::id();
    let mint_b_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;
    let lp_mint_kp = create_mint(&mut svm, &payer, &mint_authority.pubkey())?;

    let mint_b = mint_b_kp.pubkey();
    let lp_mint = lp_mint_kp.pubkey();
    println!("Mint A (wSOL): {}", mint_a);
    println!("Mint B (SPL): {}", mint_b);
    println!("Mint LP (SPL): {}", lp_mint);

    let (sorted_mint_a, sorted_mint_b) = if mint_a < mint_b {
        (mint_a, mint_b)
    } else {
        (mint_b, mint_a)
    };

    let plugin_state_kp = Keypair::new();
    let plugin_state_pk = plugin_state_kp.pubkey();
    let plugin_state_size = size_of::<PluginCalcResult>();
    let rent = svm.get_sysvar::<Rent>();
    let plugin_state_rent = rent.minimum_balance(plugin_state_size);
    let create_plugin_state_ix = system_instruction::create_account(
        &payer.pubkey(),
        &plugin_state_pk,
        plugin_state_rent,
        plugin_state_size as u64,
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

    let vault_a_pk = get_associated_token_address(&pool_pda, &mint_a);
    let vault_b_pk = get_associated_token_address(&pool_pda, &mint_b);
    println!("Derived Vault A (wSOL) ATA: {}", vault_a_pk);
    println!("Derived Vault B (SPL) ATA: {}", vault_b_pk);

    // Create Vault ATAs using the ATA instruction
    // --- SPLIT TX 1: Create wSOL Vault ATA ---
    let create_ata_a_ix = create_associated_token_account(
        &payer.pubkey(),
        &pool_pda, // Pool PDA is the owner of the vaults
        &mint_a,   // wSOL mint
        &spl_token::id(),
    );

    // --- Debugging wSOL ATA Creation ---
    println!("--- Debugging wSOL Vault ATA Creation (Setup Tx 1) ---");
    println!("Payer: {}", payer.pubkey());
    println!("Pool PDA (Owner): {}", pool_pda);
    println!("wSOL Mint (mint_a): {}", mint_a);
    println!("SPL Token Program ID: {}", spl_token::id());
    println!(
        "Associated Token Program ID: {}",
        spl_associated_token_account::id()
    );

    // Check the mint account in the SVM
    let mint_account_info = svm.get_account(&mint_a);
    println!("wSOL Mint Account Info: {:?}", mint_account_info);
    if let Some(acc) = mint_account_info {
        println!("wSOL Mint Account Owner: {}", acc.owner);
        // We expect the owner to be the SPL Token program ID for spl-token functions to work
        assert_eq!(
            acc.owner,
            spl_token::id(),
            "wSOL mint account owner is not the SPL Token program!"
        );
    } else {
        // The native mint should inherently exist in a Solana environment
        println!("CRITICAL: wSOL Mint Account {} not found in SVM!", mint_a);
        panic!("Native wSOL mint account missing from LiteSVM environment.");
    }
    println!("wSOL Mint account owner check passed.");
    // --- End Debugging ---

    let setup_tx_1 = Transaction::new_signed_with_payer(
        &[create_ata_a_ix],
        Some(&payer.pubkey()),
        &[&payer], // Payer funds rent
        svm.latest_blockhash(),
    );
    println!("Sending setup transaction 1 (wSOL Vault ATA)... ");
    map_litesvm_err(svm.send_transaction(setup_tx_1))?;
    println!("Setup transaction 1 successful.");

    // --- SPLIT TX 2: Create SPL Vault ATA and Set LP Auth ---
    let create_ata_b_ix = create_associated_token_account(
        &payer.pubkey(),
        &pool_pda, // Pool PDA is the owner of the vaults
        &mint_b,   // SPL mint
        &spl_token::id(),
    );
    // Set LP Mint Authority to Pool PDA
    let set_lp_auth_ix = spl_token::instruction::set_authority(
        &spl_token::id(),
        &lp_mint,
        Some(&pool_pda),
        spl_token::instruction::AuthorityType::MintTokens,
        &mint_authority.pubkey(),
        &[&mint_authority.pubkey()],
    )?;

    let setup_tx_2 = Transaction::new_signed_with_payer(
        &[create_ata_b_ix, set_lp_auth_ix],
        Some(&payer.pubkey()),
        &[&payer, &mint_authority],
        svm.latest_blockhash(),
    );
    println!("Sending setup transaction 2 (SPL Vault ATA & LP Auth)... ");
    map_litesvm_err(svm.send_transaction(setup_tx_2))?;
    println!("Setup transaction 2 successful.");

    let init_ix = Instruction {
        program_id: dex_pid,
        accounts: vec![
            AccountMeta::new(payer.pubkey(), true),
            AccountMeta::new(pool_pda, false),
            AccountMeta::new(vault_a_pk, false),
            AccountMeta::new(vault_b_pk, false),
            AccountMeta::new(lp_mint, false),
            AccountMeta::new_readonly(mint_a, false),
            AccountMeta::new_readonly(mint_b, false),
            AccountMeta::new_readonly(plugin_pid, false),
            AccountMeta::new(plugin_state_pk, false),
            AccountMeta::new_readonly(system_program::id(), false),
            AccountMeta::new_readonly(sysvar::rent::id(), false),
            AccountMeta::new_readonly(spl_token::id(), false),
        ],
        data: PoolInstruction::InitializePool.try_to_vec()?,
    };
    let init_pool_tx = Transaction::new_signed_with_payer(
        &[init_ix],
        Some(&payer.pubkey()),
        &[&payer],
        svm.latest_blockhash(),
    );
    println!("Sending InitializePool transaction...");
    map_litesvm_err(svm.send_transaction(init_pool_tx))?;
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

// Helper function to execute a swap generically
fn execute_swap(
    setup: &mut TestSetup,
    swapper_kp: &Keypair,
    source_ata: &Pubkey,      // User's source ATA (e.g., wSOL or SPL B)
    destination_ata: &Pubkey, // User's destination ATA (e.g., SPL B or wSOL)
    amount_in: u64,
    min_out: u64,
) -> Result<(), Box<dyn Error>> {
    // Determine if it's A->B or B->A based on source ATA mint
    // NOTE: This relies on TestSetup having mint_a/mint_b correctly assigned
    let source_account = setup.svm.get_account(source_ata).ok_or_else(|| {
        Box::<dyn Error>::from(format!("Swap source ATA {} not found", source_ata))
    })?;
    let source_token_account = spl_token::state::Account::unpack(&source_account.data)?;

    let (vault_in, vault_out) = if source_token_account.mint == setup.mint_a {
        // Swapping A for B
        (setup.vault_a_pk, setup.vault_b_pk)
    } else if source_token_account.mint == setup.mint_b {
        // Swapping B for A
        (setup.vault_b_pk, setup.vault_a_pk)
    } else {
        return Err(Box::<dyn Error>::from(
            "Swap source ATA mint does not match pool mints",
        ));
    };

    let swap_ix = Instruction {
        program_id: setup.dex_pid,
        accounts: vec![
            AccountMeta::new(swapper_kp.pubkey(), true), // 0 User swapper signer
            AccountMeta::new(setup.pool_pda, false),     // 1 Pool state
            AccountMeta::new(setup.vault_a_pk, false),   // 2 Vault A (matches pool_data.vault_a)
            AccountMeta::new(setup.vault_b_pk, false),   // 3 Vault B (matches pool_data.vault_b)
            AccountMeta::new(*source_ata, false),        // 4 User Source ATA
            AccountMeta::new(*destination_ata, false),   // 5 User Destination ATA
            AccountMeta::new_readonly(spl_token::id(), false), // 6 Token Program
            AccountMeta::new_readonly(setup.plugin_pid, false), // 7 Plugin Program
            AccountMeta::new(setup.plugin_state_pk, false), // 8 Plugin State
        ],
        // Use correct fields for Swap instruction
        data: PoolInstruction::Swap { amount_in, min_out }.try_to_vec()?,
    };

    let tx = Transaction::new_signed_with_payer(
        &[swap_ix],
        Some(&setup.payer.pubkey()), // Setup payer pays fees
        &[&setup.payer, swapper_kp], // Payer + Swapper sign
        setup.svm.latest_blockhash(),
    );
    map_litesvm_err(setup.svm.send_transaction(tx))?;
    Ok(())
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
            AccountMeta::new(setup.pool_pda, false), // Writable=false OK
            AccountMeta::new(setup.vault_a_pk, false), // Writable=false OK
            AccountMeta::new(setup.vault_b_pk, false), // Writable=false OK
            AccountMeta::new(setup.lp_mint, false),  // Writable=false OK
            AccountMeta::new_readonly(setup.mint_a, false),
            AccountMeta::new_readonly(setup.mint_b, false),
            AccountMeta::new_readonly(setup.plugin_pid, false),
            AccountMeta::new(setup.plugin_state_pk, false), // Writable=false OK
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
    println!(
        "Re-Init Message Header: ({}, {}, {})",
        message.header.num_required_signatures,
        message.header.num_readonly_signed_accounts,
        message.header.num_readonly_unsigned_accounts
    );

    let mut tx = Transaction::new_unsigned(message); // Create unsigned
    println!(
        "Attempting to sign Re-Init Tx with Payer: {}",
        setup.payer.pubkey()
    );
    println!("Blockhash for Re-Init signing: {}", latest_blockhash);
    tx.sign(&[&setup.payer], latest_blockhash); // Sign
    println!("Re-Init Transaction signed successfully (apparently).");

    println!("Sending Re-InitializePool transaction...");
    let result: Result<TransactionMetadata, FailedTransactionMetadata> =
        setup.svm.send_transaction(tx);
    assert!(result.is_err(), "Re-initialize TX should fail");

    // Extract the FailedTransactionMetadata since we asserted it's Err
    let failed_metadata = result.expect_err("Assertion failed: result was not Err");

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

// NEW TEST: Full wSOL cycle
#[test]
fn test_wsol_pool_full_cycle() -> Result<(), Box<dyn Error>> {
    println!("\n--- Starting test_wsol_pool_full_cycle ---");
    let mut setup = setup_wsol_test_environment()?;

    // 1. Setup User
    let user_kp = Keypair::new();
    let user_pk = user_kp.pubkey();
    map_litesvm_err(setup.svm.airdrop(&user_pk, 5 * LAMPORTS_PER_SOL))?; // Give user some SOL
    let initial_user_sol = setup.svm.get_balance(&user_pk).unwrap_or(0);
    println!("User {} Initial SOL Balance: {}", user_pk, initial_user_sol);

    // 2. Wrap SOL
    let wrap_amount = 1 * LAMPORTS_PER_SOL;
    println!("Attempting to wrap {} SOL...", wrap_amount);
    let user_wsol_ata = wrap_sol(&mut setup.svm, &setup.payer, &user_kp, wrap_amount)?;
    println!("User {} wSOL ATA: {}", user_pk, user_wsol_ata);
    let user_sol_after_wrap = setup.svm.get_balance(&user_pk).unwrap_or(0);
    assert!(user_sol_after_wrap < initial_user_sol); // SOL decreased

    // 3. Setup User's SPL Token B ATA and Mint Tokens
    let user_ata_b = create_user_ata(&mut setup.svm, &setup.payer, &user_pk, &setup.mint_b)?;
    let user_initial_b_amount = 5_000_000_000; // 5k tokens B (example)
    mint_to_ata(
        &mut setup.svm,
        &setup.payer,
        &setup.mint_authority,
        &setup.mint_b,
        &user_ata_b,
        user_initial_b_amount,
    )?;
    assert_eq!(
        get_token_balance(&setup.svm, &user_ata_b),
        user_initial_b_amount
    );
    println!("User {} ATA B: {}", user_pk, user_ata_b);
    println!("Minted {} tokens B to user", user_initial_b_amount);

    // 4. Setup User's LP Token ATA
    let user_ata_lp = create_user_ata(&mut setup.svm, &setup.payer, &user_pk, &setup.lp_mint)?;
    println!("User {} ATA LP: {}", user_pk, user_ata_lp);

    // 5. Add Liquidity (wSOL + Token B)
    // Note: The amount of wSOL available is `wrap_amount`
    let deposit_wsol = wrap_amount / 2; // Use half the wrapped SOL
    let deposit_b = 2_000_000_000; // Use some token B
    println!(
        "Attempting to add liquidity: {} wSOL, {} Token B",
        deposit_wsol, deposit_b
    );
    execute_add_liquidity(
        &mut setup,
        &user_kp,
        &user_wsol_ata, // Source A is wSOL ATA
        &user_ata_b,    // Source B is SPL ATA
        &user_ata_lp,   // Dest LP ATA
        deposit_wsol,
        deposit_b,
    )?;
    let vault_wsol_balance = get_token_balance(&setup.svm, &setup.vault_a_pk); // Vault A is wSOL
    let vault_b_balance = get_token_balance(&setup.svm, &setup.vault_b_pk); // Vault B is SPL
    let user_lp_balance = get_token_balance(&setup.svm, &user_ata_lp);
    assert_eq!(vault_wsol_balance, deposit_wsol);
    assert_eq!(vault_b_balance, deposit_b);
    assert!(user_lp_balance > 0);
    println!(
        "Liquidity Added. Vault wSOL: {}, Vault B: {}, User LP: {}",
        vault_wsol_balance, vault_b_balance, user_lp_balance
    );

    // 6. Swap wSOL -> Token B
    let swap_wsol_in = vault_wsol_balance / 4; // Swap 1/4 of vault's wSOL
    let min_b_out = 1; // Expect at least 1 token B out
    let user_wsol_before_swap1 = get_token_balance(&setup.svm, &user_wsol_ata);
    let user_b_before_swap1 = get_token_balance(&setup.svm, &user_ata_b);
    println!(
        "Attempting Swap 1 (wSOL -> B): Amount In = {}, Min Out = {}",
        swap_wsol_in, min_b_out
    );
    execute_swap(
        &mut setup,
        &user_kp,
        &user_wsol_ata, // Source is wSOL
        &user_ata_b,    // Destination is B
        swap_wsol_in,
        min_b_out,
    )?;
    let user_wsol_after_swap1 = get_token_balance(&setup.svm, &user_wsol_ata);
    let user_b_after_swap1 = get_token_balance(&setup.svm, &user_ata_b);
    assert_eq!(user_wsol_after_swap1, user_wsol_before_swap1 - swap_wsol_in);
    assert!(user_b_after_swap1 > user_b_before_swap1); // Received some B
    println!(
        "Swap 1 Complete. User wSOL: {}, User B: {}",
        user_wsol_after_swap1, user_b_after_swap1
    );

    // 7. Swap Token B -> wSOL
    let swap_b_in = (user_b_after_swap1 - user_b_before_swap1) / 2; // Swap back half of what was received
    let min_wsol_out = 1; // Expect at least 1 lamport out
    let user_wsol_before_swap2 = user_wsol_after_swap1;
    let user_b_before_swap2 = user_b_after_swap1;
    println!(
        "Attempting Swap 2 (B -> wSOL): Amount In = {}, Min Out = {}",
        swap_b_in, min_wsol_out
    );
    execute_swap(
        &mut setup,
        &user_kp,
        &user_ata_b,    // Source is B
        &user_wsol_ata, // Destination is wSOL
        swap_b_in,
        min_wsol_out,
    )?;
    let user_wsol_after_swap2 = get_token_balance(&setup.svm, &user_wsol_ata);
    let user_b_after_swap2 = get_token_balance(&setup.svm, &user_ata_b);
    assert!(user_wsol_after_swap2 > user_wsol_before_swap2); // Received some wSOL
    assert_eq!(user_b_after_swap2, user_b_before_swap2 - swap_b_in);
    println!(
        "Swap 2 Complete. User wSOL: {}, User B: {}",
        user_wsol_after_swap2, user_b_after_swap2
    );

    // 8. Unwrap Remaining wSOL
    let wsol_to_unwrap = get_token_balance(&setup.svm, &user_wsol_ata);
    println!("Attempting to unwrap {} wSOL...", wsol_to_unwrap);
    if wsol_to_unwrap > 0 {
        unwrap_wsol(&mut setup.svm, &setup.payer, &user_kp, &user_wsol_ata)?;
        let user_sol_after_unwrap = setup.svm.get_balance(&user_pk).unwrap_or(0);
        let wsol_account_final = setup.svm.get_account(&user_wsol_ata);
        assert_eq!(
            wsol_account_final.map(|acc| acc.lamports).unwrap_or(0),
            0,
            "wSOL ATA should be closed after unwrap (zero lamports)"
        );
        println!("Unwrap Complete. Final User SOL: {}", user_sol_after_unwrap);
    } else {
        println!("Skipping unwrap as wSOL balance is zero.");
    }

    // Final checks (optional: check LP value, remove liquidity etc.)
    println!("test_wsol_pool_full_cycle Passed!");
    Ok(())
}

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
            AccountMeta::new(user_kp.pubkey(), true),  // user signer
            AccountMeta::new(setup.pool_pda, false),   // pool state
            AccountMeta::new(setup.vault_a_pk, false), // vault a
            AccountMeta::new(setup.vault_b_pk, false), // vault b
            AccountMeta::new(setup.lp_mint, false),    // lp mint
            AccountMeta::new(user_ata_a, false),       // user token A
            AccountMeta::new(user_ata_b, false),       // user token B
            AccountMeta::new(user_ata_lp, false),      // user LP
            AccountMeta::new_readonly(spl_token::id(), false), // token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // plugin state
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
    // Correct: Expect success, map error if it fails unexpectedly
    map_litesvm_err(setup.svm.send_transaction(remove_tx))?;

    // Restore original assertions for this test
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
        deposit_a * 2, // Mint more than needed
    )?;
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
            AccountMeta::new(user_kp.pubkey(), true),  // user signer
            AccountMeta::new(setup.pool_pda, false),   // pool state
            AccountMeta::new(setup.vault_a_pk, false), // vault a
            AccountMeta::new(setup.vault_b_pk, false), // vault b
            AccountMeta::new(setup.lp_mint, false),    // lp mint
            AccountMeta::new(user_ata_a, false),       // user token A
            AccountMeta::new(user_ata_b, false),       // user token B
            AccountMeta::new(user_ata_lp, false),      // user LP
            AccountMeta::new_readonly(spl_token::id(), false), // token program
            AccountMeta::new_readonly(setup.plugin_pid, false), // plugin program
            AccountMeta::new(setup.plugin_state_pk, false), // plugin state
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

    let theoretical_b_num = (vault_b_after_add as u128) * (remove_amount_lp as u128);
    let theoretical_b_out_floor = theoretical_b_num / (total_lp_after_add as u128);

    // Log amounts for comparison (using float for readability)
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
    println!("Actual received A: {}, B: {}", received_a, received_b);

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
    // Use the generic swap helper
    execute_swap(
        &mut setup,
        &swapper_kp,
        &swapper_ata_a, // Source is A
        &swapper_ata_b, // Dest is B
        amount_in,
        min_out,
    )?;

    // Assert balances
    let final_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let final_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let final_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Calculate expected output (simple CPMM with 0.3% fee)
    let effective_in = (amount_in as u128) * 997 / 1000;
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
    println!(
        "Actual Amount Out (User B change): {}",
        final_user_b - initial_user_b
    );
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
        initial_user_b + expected_amount_out as u64,
        "User B balance mismatch"
    );
    assert_eq!(
        final_vault_a,
        initial_vault_a + amount_in,
        "Vault A balance mismatch"
    );
    assert_eq!(
        final_vault_b,
        initial_vault_b - expected_amount_out as u64,
        "Vault B balance mismatch"
    );
    // Check minimum out constraint
    assert!(
        final_user_b - initial_user_b >= min_out, // Check actual received vs min_out
        "Swap output less than minimum required"
    );

    println!("Swap A->B Test Passed!");
    Ok(())
}

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
    // Use generic swap helper
    execute_swap(
        &mut setup,
        &swapper_kp,
        &swapper_ata_b, // Source is B
        &swapper_ata_a, // Dest is A
        amount_in,
        min_out,
    )?;

    // Assert balances
    let final_user_a = get_token_balance(&setup.svm, &swapper_ata_a);
    let final_user_b = get_token_balance(&setup.svm, &swapper_ata_b);
    let final_vault_a = get_token_balance(&setup.svm, &setup.vault_a_pk);
    let final_vault_b = get_token_balance(&setup.svm, &setup.vault_b_pk);

    // Calculate expected output (simple CPMM with 0.3% fee)
    let effective_in = (amount_in as u128) * 997 / 1000;
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
    println!(
        "Actual Amount Out (User A change): {}",
        final_user_a - initial_user_a
    );
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
        initial_user_a + expected_amount_out as u64,
        "User A balance mismatch"
    );
    assert_eq!(
        final_vault_b,
        initial_vault_b + amount_in,
        "Vault B balance mismatch"
    );
    assert_eq!(
        final_vault_a,
        initial_vault_a - expected_amount_out as u64,
        "Vault A balance mismatch"
    );
    // Check minimum out constraint
    assert!(
        final_user_a - initial_user_a >= min_out,
        "Swap output less than minimum required"
    );

    println!("Swap B->A Test Passed!");
    Ok(())
}

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
    let actual_deposit_a =
        (vault_a_before as u128 * deposit_b_attempt as u128 / vault_b_before as u128) as u64;
    let actual_deposit_b = deposit_b_attempt; // B is the limiting factor
    let refunded_a = deposit_a_attempt - actual_deposit_a;

    // Calculate expected LP minted based on existing pool ratio
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
