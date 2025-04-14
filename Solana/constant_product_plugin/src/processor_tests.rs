#[cfg(test)]
mod tests {
    // Note: Adjust the `use super::*;` or `use crate::...;` lines
    // depending on where your processor module and types are located.
    // Assuming they are accessible via `crate::processor::...`
    use crate::processor::{PluginCalcResult, Processor};
    use borsh::BorshDeserialize;
    use solana_program::{
        account_info::AccountInfo, clock::Epoch, program_error::ProgramError, pubkey::Pubkey,
    };
    use std::mem;

    // Helper to create a basic AccountInfo for testing state accounts
    // Lifetimes need to be specified for references within AccountInfo
    fn create_state_account_info<'a>(
        key: &'a Pubkey,
        is_writable: bool,
        lamports: &'a mut u64,
        data: &'a mut [u8],
        owner: &'a Pubkey,
    ) -> AccountInfo<'a> {
        AccountInfo::new(
            key,
            false, // is_signer
            is_writable,
            lamports,
            data,
            owner,
            false, // executable
            Epoch::default(),
        )
    }

    #[test]
    fn test_compute_add_liquidity_first_deposit() {
        let owner_program_id = Pubkey::new_unique(); // Dummy owner for account
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        // Size needs to be sufficient for PluginCalcResult serialization
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true, // Writable
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        // Pass as a slice, as the processor expects &[AccountInfo]
        let accounts = [state_acc_info];

        let reserve_a = 0u64;
        let reserve_b = 0u64;
        let deposit_a = 100u64;
        let deposit_b = 400u64;
        let total_lp_supply = 0u64;

        // Expected shares = sqrt(deposit_a * deposit_b) = sqrt(100 * 400) = sqrt(40000) = 200
        let expected_shares = 200u64;

        let result = Processor::compute_add_liquidity(
            &accounts, // Pass the slice
            reserve_a,
            reserve_b,
            deposit_a,
            deposit_b,
            total_lp_supply,
        );

        assert!(
            result.is_ok(),
            "compute_add_liquidity failed: {:?}",
            result.err()
        );

        // Check the state account data for results
        // Use `deserialize` which handles reading from the start of the buffer
        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();

        assert_eq!(calc_result.actual_a, deposit_a, "actual_a mismatch");
        assert_eq!(calc_result.actual_b, deposit_b, "actual_b mismatch");
        assert_eq!(
            calc_result.shares_to_mint, expected_shares,
            "shares_to_mint mismatch"
        );
        // Other fields should be default (0)
        assert_eq!(calc_result.withdraw_a, 0, "withdraw_a non-zero");
        assert_eq!(calc_result.withdraw_b, 0, "withdraw_b non-zero");
        assert_eq!(calc_result.amount_out, 0, "amount_out non-zero");
    }

    #[test]
    fn test_compute_add_liquidity_existing_pool() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        // Existing pool state
        let reserve_a = 1000u64;
        let reserve_b = 5000u64; // Price: 5 B per A
        let total_lp_supply = 10000u64; // Arbitrary existing LP

        // User wants to deposit 100 A and 600 B
        let deposit_a = 100u64;
        let deposit_b = 600u64;

        // Calculation: Ratio is 5000/1000 = 5 B per A.
        // Max B for 100 A is 100 * 5 = 500 B.
        // Max A for 600 B is 600 / 5 = 120 A.
        // Since user deposits 600 B > 500 B (max for 100 A), deposit_a is limiting.
        // Actual deposit should be 100 A and 500 B.
        let expected_actual_a = 100u64;
        let expected_actual_b = 500u64;

        // Expected shares = total_lp * actual_a / reserve_a
        //                = 10000 * 100 / 1000 = 1000
        let expected_shares = 1000u64;

        let result = Processor::compute_add_liquidity(
            &accounts,
            reserve_a,
            reserve_b,
            deposit_a,
            deposit_b,
            total_lp_supply,
        );
        assert!(
            result.is_ok(),
            "compute_add_liquidity (existing) failed: {:?}",
            result.err()
        );

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.actual_a, expected_actual_a,
            "existing actual_a mismatch"
        );
        assert_eq!(
            calc_result.actual_b, expected_actual_b,
            "existing actual_b mismatch"
        );
        assert_eq!(
            calc_result.shares_to_mint, expected_shares,
            "existing shares_to_mint mismatch"
        );
    }

    #[test]
    fn test_compute_remove_liquidity() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_a = 1000u64;
        let reserve_b = 5000u64;
        let total_lp_supply = 10000u64;
        let lp_amount_burning = 2000u64; // Burn 20% of LP

        // Expected withdraw = reserve * (burn / total_lp)
        let expected_withdraw_a =
            (reserve_a as u128 * lp_amount_burning as u128 / total_lp_supply as u128) as u64;
        let expected_withdraw_b =
            (reserve_b as u128 * lp_amount_burning as u128 / total_lp_supply as u128) as u64;
        // 1000 * 2000 / 10000 = 200 A
        // 5000 * 2000 / 10000 = 1000 B
        assert_eq!(expected_withdraw_a, 200);
        assert_eq!(expected_withdraw_b, 1000);

        let result = Processor::compute_remove_liquidity(
            &accounts,
            reserve_a,
            reserve_b,
            total_lp_supply,
            lp_amount_burning,
        );
        assert!(
            result.is_ok(),
            "compute_remove_liquidity failed: {:?}",
            result.err()
        );

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.withdraw_a, expected_withdraw_a,
            "remove withdraw_a mismatch"
        );
        assert_eq!(
            calc_result.withdraw_b, expected_withdraw_b,
            "remove withdraw_b mismatch"
        );
        // Other fields should be default (0)
        assert_eq!(calc_result.actual_a, 0);
        assert_eq!(calc_result.actual_b, 0);
        assert_eq!(calc_result.shares_to_mint, 0);
        assert_eq!(calc_result.amount_out, 0);
    }

    #[test]
    fn test_compute_swap() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_in = 10000u64; // Token A
        let reserve_out = 20000u64; // Token B
        let amount_in = 1000u64; // Swap 1000 A

        // Calculation with 0.3% fee:
        // fee_num = 3, fee_den = 1000
        // effective_in = 1000 * (1000 - 3) / 1000 = 1000 * 997 / 1000 = 997
        // new_in = reserve_in + effective_in = 10000 + 997 = 10997
        // amount_out = reserve_out * effective_in / new_in
        //            = 20000 * 997 / 10997 = 19940000 / 10997 = 1813 (integer division)
        let expected_amount_out = 1813u64;

        let result = Processor::compute_swap(&accounts, reserve_in, reserve_out, amount_in);
        assert!(result.is_ok(), "compute_swap failed: {:?}", result.err());

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.amount_out, expected_amount_out,
            "swap amount_out mismatch"
        );
        // Other fields should be default (0)
        assert_eq!(calc_result.actual_a, 0);
        assert_eq!(calc_result.actual_b, 0);
        assert_eq!(calc_result.shares_to_mint, 0);
        assert_eq!(calc_result.withdraw_a, 0);
        assert_eq!(calc_result.withdraw_b, 0);
    }

    #[test]
    fn test_compute_add_liquidity_zero_deposit() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        // Scenario 1: First deposit, zero amounts
        let result1 = Processor::compute_add_liquidity(&accounts, 0, 0, 0, 0, 0);
        // Expect error because sqrt(0*0) = 0 shares
        assert_eq!(result1.err(), Some(ProgramError::InvalidArgument));

        // Scenario 2: Existing pool, zero amounts
        let result2 = Processor::compute_add_liquidity(&accounts, 1000, 1000, 0, 0, 1000);
        // Expect error because shares calculated will be 0
        assert_eq!(result2.err(), Some(ProgramError::InvalidArgument));
    }

    #[test]
    fn test_compute_add_liquidity_large_numbers() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_a = u64::MAX / 2;
        let reserve_b = u64::MAX / 2;
        let deposit_a = u64::MAX / 4; // Will be limited by ratio
        let deposit_b = u64::MAX / 2; // Max possible deposit for ratio
        let total_lp_supply = u64::MAX / 2;

        // Ratio is 1:1. Max deposit_a for deposit_b is (MAX/2 * (MAX/2)) / (MAX/2) = MAX/2.
        // Max deposit_b for deposit_a is (MAX/4 * (MAX/2)) / (MAX/2) = MAX/4.
        // So, deposit_b is limiting.
        // Actual deposit will be MAX/4 A and MAX/4 B.
        let expected_actual_a = u64::MAX / 4;
        let expected_actual_b = u64::MAX / 4;
        // Shares = total_lp * actual_a / reserve_a = (MAX/2) * (MAX/4) / (MAX/2) = MAX/4
        let expected_shares = u64::MAX / 4;

        let result = Processor::compute_add_liquidity(
            &accounts,
            reserve_a,
            reserve_b,
            deposit_a,
            deposit_b,
            total_lp_supply,
        );
        assert!(
            result.is_ok(),
            "compute_add_liquidity (large) failed: {:?}",
            result.err()
        );

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.actual_a, expected_actual_a,
            "large actual_a mismatch"
        );
        assert_eq!(
            calc_result.actual_b, expected_actual_b,
            "large actual_b mismatch"
        );
        assert_eq!(
            calc_result.shares_to_mint, expected_shares,
            "large shares_to_mint mismatch"
        );
    }

    #[test]
    fn test_compute_remove_liquidity_burn_all() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_a = 12345u64;
        let reserve_b = 54321u64;
        let total_lp_supply = 10000u64;
        let lp_amount_burning = total_lp_supply; // Burn all

        let expected_withdraw_a = reserve_a; // Should get all reserves back
        let expected_withdraw_b = reserve_b;

        let result = Processor::compute_remove_liquidity(
            &accounts,
            reserve_a,
            reserve_b,
            total_lp_supply,
            lp_amount_burning,
        );
        assert!(
            result.is_ok(),
            "compute_remove_liquidity (burn all) failed: {:?}",
            result.err()
        );

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.withdraw_a, expected_withdraw_a,
            "burn all withdraw_a mismatch"
        );
        assert_eq!(
            calc_result.withdraw_b, expected_withdraw_b,
            "burn all withdraw_b mismatch"
        );
    }

    #[test]
    fn test_compute_remove_liquidity_burn_zero() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_a = 1000u64;
        let reserve_b = 5000u64;
        let total_lp_supply = 10000u64;
        let lp_amount_burning = 0u64; // Burn zero

        let result = Processor::compute_remove_liquidity(
            &accounts,
            reserve_a,
            reserve_b,
            total_lp_supply,
            lp_amount_burning,
        );
        // Code explicitly checks for burn == 0
        assert_eq!(result.err(), Some(ProgramError::InvalidArgument));
    }

    #[test]
    fn test_compute_swap_zero_input() {
        let owner_program_id = Pubkey::new_unique();
        let state_key = Pubkey::new_unique();
        let mut lamports: u64 = 0;
        let mut data: Vec<u8> = vec![0; mem::size_of::<PluginCalcResult>()];
        let state_acc_info = create_state_account_info(
            &state_key,
            true,
            &mut lamports,
            &mut data,
            &owner_program_id,
        );
        let accounts = [state_acc_info];

        let reserve_in = 10000u64;
        let reserve_out = 20000u64;
        let amount_in = 0u64;

        let expected_amount_out = 0u64;

        let result = Processor::compute_swap(&accounts, reserve_in, reserve_out, amount_in);
        assert!(
            result.is_ok(),
            "compute_swap (zero input) failed: {:?}",
            result.err()
        );

        let calc_result = PluginCalcResult::deserialize(&mut &data[..]).unwrap();
        assert_eq!(
            calc_result.amount_out, expected_amount_out,
            "swap zero input amount_out mismatch"
        );
    }

    // TODO: Add more tests for edge cases (reserve = 0 checks, potential overflows in swap/remove)
}
