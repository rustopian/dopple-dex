#[cfg(test)]
mod tests {
    // Assuming processor types are in the parent module
    use crate::{
        instruction::PoolInstruction,
        processor::{PluginCalcResult, Processor},
        state::PoolState,
    };
    use borsh::{BorshDeserialize, BorshSerialize};
    use solana_program::{
        account_info::AccountInfo,
        clock::Epoch,
        program_pack::Pack, // Import Pack for SPL states
        pubkey::Pubkey,
        sysvar::rent::Rent,
    };
    use spl_token::state::{Account as SplAccount, AccountState, Mint}; // Added AccountState, Mint, and aliased Account
    use std::mem;
    use bincode;
    use spl_associated_token_account::get_associated_token_address; // ADD import

    // Basic AccountInfo helper
    fn create_account_info<'a>(
        key: &'a Pubkey,
        is_signer: bool,
        is_writable: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8],
        owner: &'a Pubkey,
        executable: bool,
    ) -> AccountInfo<'a> {
        AccountInfo::new(
            key,
            is_signer,
            is_writable,
            lamports,
            data,
            owner,
            executable,
            Epoch::default(),
        )
    }

    // Helper to create AccountInfo for SPL Token Accounts
    fn create_token_account_info<'a>(
        key: &'a Pubkey,
        is_writable: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8],               // Should contain serialized TokenAccount
        _token_account_owner: &'a Pubkey, // Typically the user or the Pool PDA
        token_program_owner: &'a Pubkey,  // Should be spl_token::id()
        // mint: &'a Pubkey, // Mint isn't needed by create_account_info itself
        // executable: bool, // Token accounts are never executable
    ) -> AccountInfo<'a> {
        create_account_info(key, false, is_writable, lamports, data, token_program_owner, false)
    }

    // Helper to create AccountInfo for the Plugin State containing a result
    fn create_plugin_state_account_info<'a>(
        key: &'a Pubkey,
        is_writable: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8], // Should contain serialized PluginCalcResult
        owner: &'a Pubkey,  // Should be the plugin program ID
        // executable: bool, // Plugin state is not executable
    ) -> AccountInfo<'a> {
        create_account_info(key, false, is_writable, lamports, data, owner, false)
    }

    #[test]
    fn test_process_initialize_pool() {
        let program_id = Pubkey::new_unique(); // Our Pool program ID
        let payer_key = Pubkey::new_unique();
        let mint_a_key = Pubkey::new_unique();
        let mint_b_key = Pubkey::new_unique();
        let plugin_prog_key = Pubkey::new_unique();
        let plugin_state_key = Pubkey::new_unique();
        let lp_mint_key = Pubkey::new_unique();
        let system_prog_key = solana_program::system_program::id();
        let token_prog_key = spl_token::id();
        let bpf_loader_key = solana_program::bpf_loader_upgradeable::id(); // ADD BPF Loader ID

        // Derive expected pool PDA
        let (sorted_mint_a, sorted_mint_b) = if mint_a_key < mint_b_key {
            (mint_a_key, mint_b_key)
        } else {
            (mint_b_key, mint_a_key)
        };
        let seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_prog_key.as_ref(),
            plugin_state_key.as_ref(),
        ];
        let (expected_pool_pda, bump) = Pubkey::find_program_address(seeds, &program_id);

        // --- Derive Vault ATAs --- ADDED
        let vault_a_key = get_associated_token_address(&expected_pool_pda, &mint_a_key);
        let vault_b_key = get_associated_token_address(&expected_pool_pda, &mint_b_key);

        // Account setup
        let mut payer_lamports: u64 = 1_000_000_000;
        let mut pool_state_lamports: u64 = 0;
        let mut vault_a_lamports: u64 = 0;
        let mut vault_b_lamports: u64 = 0;
        let mut lp_mint_lamports: u64 = 0;
        let mut mint_a_lamports: u64 = 0;
        let mut mint_b_lamports: u64 = 0;
        let mut plugin_state_lamports: u64 = 0;
        let mut plugin_prog_lamports: u64 = 0;
        let mut system_lamports: u64 = 0;
        let mut rent_lamports: u64 = 1_000_000;

        // Calculate serialized size instead of using mem::size_of
        let dummy_pool_state = PoolState {
            token_mint_a: mint_a_key,
            token_mint_b: mint_b_key,
            vault_a: vault_a_key,
            vault_b: vault_b_key,
            lp_mint: lp_mint_key,
            total_lp_supply: 0,
            bump, // Use derived bump
            plugin_program_id: plugin_prog_key,
            plugin_state_pubkey: plugin_state_key,
        };
        let pool_state_data_bytes = dummy_pool_state.try_to_vec().unwrap();
        let pool_state_size = pool_state_data_bytes.len(); // Get actual serialized size

        // Allocate buffer with the correct serialized size
        let mut pool_state_data: Vec<u8> = vec![0; pool_state_size];

        // Allocate Rent data buffer, default zeroed data is usually fine for tests
        let rent_size = mem::size_of::<Rent>();
        let mut rent_data: Vec<u8> = vec![0; rent_size];
        // No need to manually serialize Rent::default() here

        let initial_reserve_a = 1000u64;
        let initial_reserve_b = 5000u64;

        let vault_a_token_state = spl_token::state::Account {
            mint: mint_a_key,
            owner: expected_pool_pda,
            amount: 0,
            state: spl_token::state::AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_a_data: Vec<u8> = vec![0; spl_token::state::Account::LEN];
        vault_a_token_state.pack_into_slice(&mut vault_a_data);

        let vault_b_token_state = spl_token::state::Account {
            mint: mint_b_key,
            owner: expected_pool_pda,
            amount: 0,
            state: spl_token::state::AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_b_data: Vec<u8> = vec![0; spl_token::state::Account::LEN];
        vault_b_token_state.pack_into_slice(&mut vault_b_data);

        let mut dummy_data_payer: Vec<u8> = vec![];
        let mut dummy_data_lp_mint: Vec<u8> = vec![];
        let mut dummy_data_mint_a: Vec<u8> = vec![];
        let mut dummy_data_mint_b: Vec<u8> = vec![];
        let mut dummy_data_plugin_prog: Vec<u8> = vec![];
        let mut dummy_data_plugin_state: Vec<u8> = vec![];
        let mut dummy_data_system: Vec<u8> = vec![];

        // --- Correctly Create Rent Data ---
        let rent = Rent::default(); // Get default Rent sysvar
        let rent_size = std::mem::size_of::<Rent>(); // Get size
        let mut rent_data = vec![0; rent_size]; // Allocate buffer
        bincode::serialize_into(&mut rent_data[..], &rent)
            .expect("Failed to serialize Rent"); // Serialize into buffer

        // --- Token Program Dummy Data (not needed for init, but account must exist) ---
        let mut dummy_data_token_prog: Vec<u8> = vec![];

        // Declare lamports variable for Token Program account
        let mut token_prog_lamports: u64 = 0;

        let spl_token_program_id = spl_token::id();
        let payer_acc = create_account_info(
            &payer_key,
            true,
            true,
            &mut payer_lamports,
            &mut dummy_data_payer,
            &system_prog_key,
            false,
        );
        let pool_state_acc = create_account_info(
            &expected_pool_pda,
            false,
            true, // POOL STATE: Writable
            &mut pool_state_lamports,
            &mut pool_state_data,
            &program_id,
            false,
        );
        let vault_a_acc = create_account_info(
            &vault_a_key, // Use derived key
            false,
            true, // VAULT A: Writable
            &mut vault_a_lamports,
            &mut vault_a_data,
            &token_prog_key, // Use variable for owner
            false,
        );
        let vault_b_acc = create_account_info(
            &vault_b_key, // Use derived key
            false,
            true, // VAULT B: Writable
            &mut vault_b_lamports,
            &mut vault_b_data,
            &token_prog_key, // Use variable for owner
            false,
        );
        let lp_mint_acc = create_account_info(
            &lp_mint_key,
            false,
            true, // LP MINT: Writable
            &mut lp_mint_lamports,
            &mut dummy_data_lp_mint,
            &spl_token_program_id,
            false,
        );
        let mint_a_acc = create_account_info(
            &mint_a_key,
            false,
            false,
            &mut mint_a_lamports,
            &mut dummy_data_mint_a,
            &spl_token_program_id,
            false,
        );
        let mint_b_acc = create_account_info(
            &mint_b_key,
            false,
            false,
            &mut mint_b_lamports,
            &mut dummy_data_mint_b,
            &spl_token_program_id,
            false,
        );
        let plugin_prog_acc = create_account_info(
            &plugin_prog_key,
            false,
            false,
            &mut plugin_prog_lamports,
            &mut dummy_data_plugin_prog,
            &bpf_loader_key, // Owner must be BPF Loader
            true, // Executable must be true
        );
        let plugin_state_acc = create_plugin_state_account_info(
            &plugin_state_key,
            true, // PLUGIN STATE: Writable
            &mut plugin_state_lamports,
            &mut dummy_data_plugin_state,
            &plugin_prog_key, // Plugin state owned by plugin program
            // executable is false by default in helper
        );
        let system_acc = create_account_info(
            &system_prog_key,
            false,
            false,
            &mut system_lamports,
            &mut dummy_data_system,
            &system_prog_key,
            false,
        );
        let rent_key = solana_program::sysvar::rent::id();
        let rent_acc = create_account_info(
            &rent_key,
            false, // is_signer
            false, // is_writable
            &mut rent_lamports,
            &mut rent_data, // Use packed rent data
            &system_prog_key, // Owner
            false,
        );

        // --- ADD Token Program Account Info ---
        let token_prog_acc = create_account_info(
            &token_prog_key, // Use variable
            false, // is_signer
            false, // is_writable
            &mut token_prog_lamports,
            &mut dummy_data_token_prog,
            &system_prog_key,
            false, // not executable
        );

        let accounts = vec![
            payer_acc,        // 0
            pool_state_acc,   // 1
            vault_a_acc,      // 2
            vault_b_acc,      // 3
            lp_mint_acc,      // 4
            mint_a_acc,       // 5
            mint_b_acc,       // 6
            plugin_prog_acc,  // 7
            plugin_state_acc, // 8
            system_acc,       // 9
            rent_acc,         // 10
            token_prog_acc,   // 11 - ADDED
        ];

        let instruction_data = PoolInstruction::InitializePool.try_to_vec().unwrap();

        let result = Processor::process(&program_id, &accounts, &instruction_data);

        assert!(
            result.is_ok(),
            "process_initialize_pool failed: {:?}",
            result.err()
        );

        // Verify state written to pool_state_data
        let pool_data = PoolState::deserialize(&mut &pool_state_data[..]).unwrap();

        assert_eq!(pool_data.token_mint_a, mint_a_key);
        assert_eq!(pool_data.token_mint_b, mint_b_key);
        assert_eq!(pool_data.vault_a, vault_a_key);
        assert_eq!(pool_data.vault_b, vault_b_key);
        assert_eq!(pool_data.lp_mint, lp_mint_key);
        assert_eq!(pool_data.total_lp_supply, 0);
        assert_eq!(pool_data.bump, bump);
        assert_eq!(pool_data.plugin_program_id, plugin_prog_key);
        assert_eq!(pool_data.plugin_state_pubkey, plugin_state_key);
    }

    #[test]
    fn test_process_add_liquidity() {
        let program_id = Pubkey::new_unique();
        let user_key = Pubkey::new_unique();
        let mint_a_key = Pubkey::new_unique();
        let mint_b_key = Pubkey::new_unique();
        let vault_a_key = Pubkey::new_unique();
        let vault_b_key = Pubkey::new_unique();
        let lp_mint_key = Pubkey::new_unique();
        let user_token_a_key = Pubkey::new_unique();
        let user_token_b_key = Pubkey::new_unique();
        let user_lp_key = Pubkey::new_unique();
        let plugin_prog_key = Pubkey::new_unique();
        let plugin_state_key = Pubkey::new_unique();
        let token_prog_key = spl_token::id();
        let spl_token_program_id = spl_token::id(); // Ensure defined
        let system_prog_key = solana_program::system_program::id(); // Ensure defined

        // Derive Pool PDA (needed for PoolState)
        let (sorted_mint_a, sorted_mint_b) = if mint_a_key < mint_b_key {
            (mint_a_key, mint_b_key)
        } else {
            (mint_b_key, mint_a_key)
        };
        let seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_prog_key.as_ref(),
            plugin_state_key.as_ref(),
        ];
        let (pool_pda, bump) = Pubkey::find_program_address(seeds, &program_id);

        // --- Initial Account States ---
        let initial_total_lp = 500u64;
        let initial_reserve_a = 1000u64;
        let initial_reserve_b = 5000u64;

        // Lamports definitions
        let mut pool_state_lamports: u64 = 1_000_000;
        let mut vault_a_lamports: u64 = 1_000_000;
        let mut vault_b_lamports: u64 = 1_000_000;
        let mut user_lamports: u64 = 1_000_000;
        let mut user_token_a_lamports: u64 = 1_000_000;
        let mut user_token_b_lamports: u64 = 1_000_000;
        let mut user_lp_lamports: u64 = 1_000_000;
        let mut lp_mint_lamports: u64 = 1_000_000;
        let mut plugin_prog_lamports: u64 = 1_000_000;
        let mut token_prog_lamports: u64 = 1_000_000;
        let mut plugin_state_lamports: u64 = 1_000_000;

        // Data buffer definitions
        let initial_pool_state = PoolState {
            token_mint_a: mint_a_key,
            token_mint_b: mint_b_key,
            vault_a: vault_a_key,
            vault_b: vault_b_key,
            lp_mint: lp_mint_key,
            total_lp_supply: initial_total_lp,
            bump,
            plugin_program_id: plugin_prog_key,
            plugin_state_pubkey: plugin_state_key,
        };
        // Use Vec::new() and serialize into it to ensure size
        let mut pool_state_data = Vec::new();
        initial_pool_state.serialize(&mut pool_state_data).unwrap();

        // Define plugin state data
        let shares_to_mint_result = 100u64;
        let plugin_result = PluginCalcResult {
            actual_a: 20,
            actual_b: 100,
            shares_to_mint: shares_to_mint_result,
            withdraw_a: 0,
            withdraw_b: 0,
            amount_out: 0,
        };
        let mut plugin_state_data = plugin_result.try_to_vec().unwrap();
        plugin_state_data.resize(200, 0);

        // Define and pack vault data HERE
        let vault_a_token_state = spl_token::state::Account {
            amount: initial_reserve_a,
            state: spl_token::state::AccountState::Initialized, // Explicitly set state
            ..Default::default()
        };
        let mut vault_a_data: Vec<u8> = vec![0; spl_token::state::Account::LEN];
        vault_a_token_state.pack_into_slice(&mut vault_a_data);

        let vault_b_token_state = spl_token::state::Account {
            amount: initial_reserve_b,
            state: spl_token::state::AccountState::Initialized, // Explicitly set state
            ..Default::default()
        };
        let mut vault_b_data: Vec<u8> = vec![0; spl_token::state::Account::LEN];
        vault_b_token_state.pack_into_slice(&mut vault_b_data);
        // END vault data definition

        // Define dummy data buffers
        let mut dummy_data_user_a: Vec<u8> = vec![];
        let mut dummy_data_user_b: Vec<u8> = vec![];
        let mut dummy_data_user_lp: Vec<u8> = vec![];
        let mut dummy_data_lp_mint: Vec<u8> = vec![];
        let mut dummy_data_plugin_prog: Vec<u8> = vec![];
        let mut dummy_data_token_prog: Vec<u8> = vec![];
        let mut dummy_data_payer: Vec<u8> = vec![];

        // --- Create AccountInfos (Corrected Again) ---
        let user_acc = create_account_info(
            &user_key,
            true,
            true,
            &mut user_lamports,
            &mut dummy_data_payer,
            &system_prog_key,
            false, // not executable
        ); // Payer owner system
        let pool_state_acc = create_account_info(
            &pool_pda,
            false,
            true,
            &mut pool_state_lamports,
            &mut pool_state_data,
            &program_id,
            false, // not executable
        );
        let vault_a_acc = create_account_info(
            &vault_a_key,
            false,
            true,
            &mut vault_a_lamports,
            &mut vault_a_data,
            &token_prog_key,
            false, // not executable
        );
        let vault_b_acc = create_account_info(
            &vault_b_key,
            false,
            true,
            &mut vault_b_lamports,
            &mut vault_b_data,
            &token_prog_key,
            false, // not executable
        );
        let lp_mint_acc = create_account_info(
            &lp_mint_key,
            false,
            true,
            &mut lp_mint_lamports,
            &mut dummy_data_lp_mint,
            &spl_token_program_id,
            false, // not executable
        );

        let user_token_a_acc = create_token_account_info(
            &user_token_a_key,
            true,
            &mut user_token_a_lamports,
            &mut dummy_data_user_a,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let user_token_b_acc = create_token_account_info(
            &user_token_b_key,
            true,
            &mut user_token_b_lamports,
            &mut dummy_data_user_b,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let user_lp_acc = create_token_account_info(
            &user_lp_key,
            true,
            &mut user_lp_lamports,
            &mut dummy_data_user_lp,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let token_prog_acc = create_account_info(
            &token_prog_key,
            false,
            false,
            &mut token_prog_lamports,
            &mut dummy_data_token_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_prog_acc = create_account_info(
            &plugin_prog_key,
            false,
            false,
            &mut plugin_prog_lamports,
            &mut dummy_data_plugin_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_state_acc = create_plugin_state_account_info(
            &plugin_state_key,
            true,
            &mut plugin_state_lamports,
            &mut plugin_state_data,
            &plugin_prog_key,
            // executable is false by default in helper
        );

        let accounts = vec![
            user_acc,         // 0
            pool_state_acc,   // 1
            vault_a_acc,      // 2
            vault_b_acc,      // 3
            lp_mint_acc,      // 4
            user_token_a_acc, // 5
            user_token_b_acc, // 6
            user_lp_acc,      // 7
            token_prog_acc,   // 8
            plugin_prog_acc,  // 9
            plugin_state_acc, // 10
        ];

        // --- Execute in separate scope to drop accounts/borrows before final check ---
        {
            let instruction_data = PoolInstruction::AddLiquidity {
                amount_a: 100, // These amounts don't directly affect the tested logic
                amount_b: 500, // as plugin result is mocked
            }
            .try_to_vec()
            .unwrap();

            let result = Processor::process(&program_id, &accounts, &instruction_data);
            assert!(
                result.is_ok(),
                "process_add_liquidity failed: {:?}",
                result.err()
            );
        }

        // --- Verify (accounts vector is dropped, mutable borrow released) ---
        // DEBUG: Check buffer state before deserializing
        // let data_slice = accounts[1].data.borrow(); // Cannot use accounts[1] anymore
        println!("DEBUG: Buffer length: {}", pool_state_data.len()); // Check original vector
        println!(
            "DEBUG: Expected PoolState size: {}",
            std::mem::size_of::<PoolState>()
        );
        // Print first few bytes (if buffer isn't empty)
        if !pool_state_data.is_empty() {
            println!(
                "DEBUG: Buffer start bytes: {:?}",
                &pool_state_data[..std::cmp::min(pool_state_data.len(), 16)]
            );
        }
        // END DEBUG

        // Verify pool state update (total_lp_supply)
        // Deserialize directly from the original vector, not the potentially reset AccountInfo borrow
        let final_pool_state = PoolState::deserialize(&mut &pool_state_data[..]).unwrap();
        let expected_total_lp = initial_total_lp.checked_add(shares_to_mint_result).unwrap();
        assert_eq!(
            final_pool_state.total_lp_supply, expected_total_lp,
            "total_lp_supply mismatch after add"
        );
    }

    #[test]
    fn test_process_remove_liquidity() {
        let program_id = Pubkey::new_unique();
        let user_key = Pubkey::new_unique();
        let mint_a_key = Pubkey::new_unique();
        let mint_b_key = Pubkey::new_unique();
        let vault_a_key = Pubkey::new_unique();
        let vault_b_key = Pubkey::new_unique();
        let lp_mint_key = Pubkey::new_unique();
        let user_token_a_key = Pubkey::new_unique();
        let user_token_b_key = Pubkey::new_unique();
        let user_lp_key = Pubkey::new_unique();
        let plugin_prog_key = Pubkey::new_unique();
        let plugin_state_key = Pubkey::new_unique();
        let token_prog_key = spl_token::id();
        let spl_token_program_id = spl_token::id();
        let system_prog_key = solana_program::system_program::id();

        let (sorted_mint_a, sorted_mint_b) = if mint_a_key < mint_b_key {
            (mint_a_key, mint_b_key)
        } else {
            (mint_b_key, mint_a_key)
        };
        let seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_prog_key.as_ref(),
            plugin_state_key.as_ref(),
        ];
        let (pool_pda, bump) = Pubkey::find_program_address(seeds, &program_id);

        // --- Initial Account States ---
        let initial_total_lp = 1000u64;
        let amount_lp_to_remove = 100u64;

        // Lamports definitions
        let mut pool_state_lamports: u64 = 1_000_000;
        let mut vault_a_lamports: u64 = 1_000_000;
        let mut vault_b_lamports: u64 = 1_000_000;
        let mut user_lamports: u64 = 1_000_000;
        let mut user_token_a_lamports: u64 = 1_000_000;
        let mut user_token_b_lamports: u64 = 1_000_000;
        let mut user_lp_lamports: u64 = 1_000_000;
        let mut lp_mint_lamports: u64 = 1_000_000;
        let mut plugin_prog_lamports: u64 = 1_000_000;
        let mut token_prog_lamports: u64 = 1_000_000;
        let mut plugin_state_lamports: u64 = 1_000_000;

        // Data buffer definitions
        let initial_pool_state = PoolState {
            token_mint_a: mint_a_key,
            token_mint_b: mint_b_key,
            vault_a: vault_a_key,
            vault_b: vault_b_key,
            lp_mint: lp_mint_key,
            total_lp_supply: initial_total_lp,
            bump,
            plugin_program_id: plugin_prog_key,
            plugin_state_pubkey: plugin_state_key,
        };
        let mut pool_state_data = initial_pool_state.try_to_vec().unwrap();

        // Mock plugin result
        let plugin_result = PluginCalcResult {
            withdraw_a: 20,
            withdraw_b: 100,
            ..Default::default()
        };
        let mut plugin_state_data = plugin_result.try_to_vec().unwrap();
        let plugin_state_acc_size = std::mem::size_of::<PluginCalcResult>();
        plugin_state_data.resize(plugin_state_acc_size, 0);

        // --- Create Initialized SPL States and Pack them ---

        // Vault A
        let vault_a_state = SplAccount {
            mint: mint_a_key,
            owner: pool_pda, // Vaults owned by the pool PDA
            amount: 5000,    // Example initial amount
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_a_data: Vec<u8> = vec![0; SplAccount::LEN];
        vault_a_state.pack_into_slice(&mut vault_a_data);

        // Vault B
        let vault_b_state = SplAccount {
            mint: mint_b_key,
            owner: pool_pda,
            amount: 25000, // Example initial amount
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_b_data: Vec<u8> = vec![0; SplAccount::LEN];
        vault_b_state.pack_into_slice(&mut vault_b_data);

        // User Token A Account
        let user_token_a_state = SplAccount {
            mint: mint_a_key,
            owner: user_key,
            amount: 0, // User starts with 0 before receiving withdrawal
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut user_token_a_data: Vec<u8> = vec![0; SplAccount::LEN];
        user_token_a_state.pack_into_slice(&mut user_token_a_data);

        // User Token B Account
        let user_token_b_state = SplAccount {
            mint: mint_b_key,
            owner: user_key,
            amount: 0,
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut user_token_b_data: Vec<u8> = vec![0; SplAccount::LEN];
        user_token_b_state.pack_into_slice(&mut user_token_b_data);

        // User LP Token Account (Holding the LP tokens to be removed)
        let user_lp_state = SplAccount {
            mint: lp_mint_key,
            owner: user_key,
            amount: amount_lp_to_remove, // User must have the LP tokens they're removing
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut user_lp_data: Vec<u8> = vec![0; SplAccount::LEN];
        user_lp_state.pack_into_slice(&mut user_lp_data);

        // LP Mint Account
        let lp_mint_state = Mint {
            mint_authority: Some(pool_pda).into(), // Pool PDA is typically mint authority
            supply: initial_total_lp,
            decimals: 9, // Example decimals
            is_initialized: true,
            freeze_authority: None.into(),
        };
        let mut lp_mint_data: Vec<u8> = vec![0; Mint::LEN];
        lp_mint_state.pack_into_slice(&mut lp_mint_data);

        // Other Dummy data buffers (can remain empty or zeroed if not unpacked)
        let mut dummy_data_plugin_prog: Vec<u8> = vec![];
        let mut dummy_data_token_prog: Vec<u8> = vec![];
        let mut dummy_data_payer: Vec<u8> = vec![];

        // --- Create AccountInfos (using packed data) ---
        let user_acc = create_account_info(
            &user_key,
            true,
            true,
            &mut user_lamports,
            &mut dummy_data_payer,
            &system_prog_key,
            false, // not executable
        );
        let pool_state_acc = create_account_info(
            &pool_pda,
            false,
            true,
            &mut pool_state_lamports,
            &mut pool_state_data,
            &program_id,
            false, // not executable
        );
        let vault_a_acc = create_account_info(
            // Use actual vault data
            &vault_a_key,
            false,
            true,
            &mut vault_a_lamports,
            &mut vault_a_data,
            &token_prog_key,
            false, // not executable
        );
        let vault_b_acc = create_account_info(
            // Use actual vault data
            &vault_b_key,
            false,
            true,
            &mut vault_b_lamports,
            &mut vault_b_data,
            &token_prog_key,
            false, // not executable
        );
        let lp_mint_acc = create_account_info(
            // Use actual mint data
            &lp_mint_key,
            false,
            true,
            &mut lp_mint_lamports,
            &mut lp_mint_data,
            &spl_token_program_id,
            false, // not executable
        );
        let user_token_a_acc = create_token_account_info(
            // Use actual user token data
            &user_token_a_key,
            true,
            &mut user_token_a_lamports,
            &mut user_token_a_data,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let user_token_b_acc = create_token_account_info(
            // Use actual user token data
            &user_token_b_key,
            true,
            &mut user_token_b_lamports,
            &mut user_token_b_data,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let user_lp_acc = create_token_account_info(
            // Use actual user LP data
            &user_lp_key,
            true,
            &mut user_lp_lamports,
            &mut user_lp_data,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let token_prog_acc = create_account_info(
            &token_prog_key,
            false,
            false,
            &mut token_prog_lamports,
            &mut dummy_data_token_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_prog_acc = create_account_info(
            &plugin_prog_key,
            false,
            false,
            &mut plugin_prog_lamports,
            &mut dummy_data_plugin_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_state_acc = create_plugin_state_account_info(
            &plugin_state_key,
            true,
            &mut plugin_state_lamports,
            &mut plugin_state_data,
            &plugin_prog_key,
            // executable is false by default in helper
        );

        let accounts = vec![
            user_acc,         // 0
            pool_state_acc,   // 1
            vault_a_acc,      // 2
            vault_b_acc,      // 3
            lp_mint_acc,      // 4
            user_token_a_acc, // 5
            user_token_b_acc, // 6
            user_lp_acc,      // 7
            token_prog_acc,   // 8
            plugin_prog_acc,  // 9
            plugin_state_acc, // 10
        ];

        // --- Execute ---
        {
            let instruction_data = PoolInstruction::RemoveLiquidity {
                amount_lp: amount_lp_to_remove,
            }
            .try_to_vec()
            .unwrap();

            let result = Processor::process(&program_id, &accounts, &instruction_data);
            // This test assumes the processor logic itself (excluding CPIs) is sound
            assert!(
                result.is_ok(),
                "process_remove_liquidity failed: {:?}",
                result.err()
            );
        }

        // --- Verify ---
        // Verify pool state update (total_lp_supply)
        let final_pool_state = PoolState::deserialize(&mut &pool_state_data[..]).unwrap();
        let expected_total_lp = initial_total_lp.checked_sub(amount_lp_to_remove).unwrap();
        assert_eq!(
            final_pool_state.total_lp_supply, expected_total_lp,
            "total_lp_supply mismatch after remove"
        );
    }

    #[test]
    fn test_process_swap() {
        let program_id = Pubkey::new_unique();
        let user_key = Pubkey::new_unique();
        let mint_a_key = Pubkey::new_unique();
        let mint_b_key = Pubkey::new_unique();
        let vault_a_key = Pubkey::new_unique();
        let vault_b_key = Pubkey::new_unique();
        let lp_mint_key = Pubkey::new_unique();
        let user_src_key = Pubkey::new_unique(); // User's source ATA (e.g., Token A)
        let user_dst_key = Pubkey::new_unique(); // User's destination ATA (e.g., Token B)
        let plugin_prog_key = Pubkey::new_unique();
        let plugin_state_key = Pubkey::new_unique();
        let token_prog_key = spl_token::id();
        let system_prog_key = solana_program::system_program::id();

        let (sorted_mint_a, sorted_mint_b) = if mint_a_key < mint_b_key {
            (mint_a_key, mint_b_key)
        } else {
            (mint_b_key, mint_a_key)
        };
        let seeds = &[
            b"pool",
            sorted_mint_a.as_ref(),
            sorted_mint_b.as_ref(),
            plugin_prog_key.as_ref(),
            plugin_state_key.as_ref(),
        ];
        let (pool_pda, bump) = Pubkey::find_program_address(seeds, &program_id);

        // --- Initial Account States ---
        let initial_total_lp = 1000u64; // Needs some LP supply for context
        let initial_reserve_a = 10000u64;
        let initial_reserve_b = 50000u64;

        // Lamports definitions (simplified)
        let mut pool_state_lamports: u64 = 1_000_000;
        let mut vault_a_lamports: u64 = 1_000_000;
        let mut vault_b_lamports: u64 = 1_000_000;
        let mut user_lamports: u64 = 1_000_000;
        let mut user_src_lamports: u64 = 1_000_000;
        let mut user_dst_lamports: u64 = 1_000_000;
        let mut plugin_prog_lamports: u64 = 1_000_000;
        let mut token_prog_lamports: u64 = 1_000_000;
        let mut plugin_state_lamports: u64 = 1_000_000;

        // Data buffer definitions
        let initial_pool_state = PoolState {
            token_mint_a: mint_a_key,
            token_mint_b: mint_b_key,
            vault_a: vault_a_key,
            vault_b: vault_b_key,
            lp_mint: lp_mint_key, // Added for PoolState struct
            total_lp_supply: initial_total_lp,
            bump,
            plugin_program_id: plugin_prog_key,
            plugin_state_pubkey: plugin_state_key,
        };
        let mut pool_state_data = initial_pool_state.try_to_vec().unwrap();

        // Mock plugin result (only amount_out matters for this test scope)
        let plugin_result = PluginCalcResult {
            amount_out: 450,
            ..Default::default()
        }; // Example amount_out
        let mut plugin_state_data = plugin_result.try_to_vec().unwrap();
        let plugin_state_acc_size = std::mem::size_of::<PluginCalcResult>();
        plugin_state_data.resize(plugin_state_acc_size, 0);

        // Define and pack vault data (needed for identifying swap direction)
        let vault_a_token_state = SplAccount {
            amount: initial_reserve_a,
            mint: mint_a_key,
            owner: pool_pda, // Vaults owned by pool PDA
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_a_data: Vec<u8> = vec![0; SplAccount::LEN];
        vault_a_token_state.pack_into_slice(&mut vault_a_data);

        let vault_b_token_state = SplAccount {
            amount: initial_reserve_b,
            mint: mint_b_key,
            owner: pool_pda,
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut vault_b_data: Vec<u8> = vec![0; SplAccount::LEN];
        vault_b_token_state.pack_into_slice(&mut vault_b_data);

        // Define user source account data (swap A -> B)
        let user_src_token_state = SplAccount {
            amount: 100, // Amount user is swapping in
            mint: mint_a_key,
            owner: user_key,
            state: AccountState::Initialized,
            ..Default::default()
        }; // Swap 100 A
        let mut user_src_data: Vec<u8> = vec![0; SplAccount::LEN];
        user_src_token_state.pack_into_slice(&mut user_src_data);

        // Define user destination account data (needs to be initialized)
        let user_dst_token_state = SplAccount {
            amount: 0,        // Starts empty before receiving swap output
            mint: mint_b_key, // Receiving token B
            owner: user_key,
            state: AccountState::Initialized,
            ..Default::default()
        };
        let mut user_dst_data: Vec<u8> = vec![0; SplAccount::LEN]; // Use actual buffer name
        user_dst_token_state.pack_into_slice(&mut user_dst_data); // Pack into it

        // Other Dummy data buffers
        let mut dummy_data_plugin_prog: Vec<u8> = vec![];
        let mut dummy_data_token_prog: Vec<u8> = vec![];
        let mut dummy_data_payer: Vec<u8> = vec![];

        // --- Create AccountInfos ---
        let user_acc = create_account_info(
            &user_key,
            true,
            true,
            &mut user_lamports,
            &mut dummy_data_payer,
            &system_prog_key,
            false, // not executable
        );
        let pool_state_acc = create_account_info(
            &pool_pda,
            false,
            true,
            &mut pool_state_lamports,
            &mut pool_state_data,
            &program_id,
            false, // not executable
        );
        let vault_a_acc = create_account_info(
            // Use actual data
            &vault_a_key,
            false,
            true,
            &mut vault_a_lamports,
            &mut vault_a_data,
            &token_prog_key,
            false, // not executable
        );
        let vault_b_acc = create_account_info(
            // Use actual data
            &vault_b_key,
            false,
            true,
            &mut vault_b_lamports,
            &mut vault_b_data,
            &token_prog_key,
            false, // not executable
        );
        let user_src_acc = create_token_account_info(
            // Use actual data
            &user_src_key,
            true,
            &mut user_src_lamports,
            &mut user_src_data,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let user_dst_acc = create_token_account_info(
            // Use actual data
            &user_dst_key,
            true,
            &mut user_dst_lamports,
            &mut user_dst_data,
            &user_key,
            &token_prog_key,
            // executable is false by default in helper
        );
        let token_prog_acc = create_account_info(
            &token_prog_key,
            false,
            false,
            &mut token_prog_lamports,
            &mut dummy_data_token_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_prog_acc = create_account_info(
            &plugin_prog_key,
            false,
            false,
            &mut plugin_prog_lamports,
            &mut dummy_data_plugin_prog,
            &system_prog_key,
            false, // not executable
        );
        let plugin_state_acc = create_plugin_state_account_info(
            &plugin_state_key,
            true,
            &mut plugin_state_lamports,
            &mut plugin_state_data,
            &plugin_prog_key,
            // executable is false by default in helper
        );

        let accounts = vec![
            user_acc,         // 0
            pool_state_acc,   // 1
            vault_a_acc,      // 2
            vault_b_acc,      // 3
            user_src_acc,     // 4
            user_dst_acc,     // 5
            token_prog_acc,   // 6
            plugin_prog_acc,  // 7
            plugin_state_acc, // 8
        ];

        // --- Execute ---
        {
            let instruction_data = PoolInstruction::Swap {
                amount_in: 100, // Matches user_src_token_state amount
                min_out: 1,     // Doesn't affect processor logic directly
            }
            .try_to_vec()
            .unwrap();

            let result = Processor::process(&program_id, &accounts, &instruction_data);
            // This test mainly verifies the processor doesn't panic when handling the swap path
            // It cannot verify the token transfers due to CPI limitations in unit tests
            assert!(result.is_ok(), "process_swap failed: {:?}", result.err());
        }

        // --- Verify ---
        // No state changes in PoolState to verify for swap in this unit test context.
        // Verification of token movements happens in integration tests.
    }
}
