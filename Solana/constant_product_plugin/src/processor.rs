use borsh::{BorshDeserialize, BorshSerialize};
use solana_program::{
    account_info::{next_account_info, AccountInfo},
    entrypoint::ProgramResult,
    msg,
    program_error::ProgramError,
    pubkey::Pubkey,
};
use spl_math::{checked_ceil_div::CheckedCeilDiv, uint::U192};
use std::convert::TryInto;

use crate::instruction::PluginInstruction;

/// We'll store the plugin's computed results in the plugin state account.
/// The pool program reads them after the CPI call.
#[derive(BorshDeserialize, BorshSerialize, Debug, Default)]
pub struct PluginCalcResult {
    pub actual_a: u64,
    pub actual_b: u64,
    pub shares_to_mint: u64,
    pub withdraw_a: u64,
    pub withdraw_b: u64,
    pub amount_out: u64,
}

pub struct Processor;
impl Processor {
    pub fn process(
        _program_id: &Pubkey,
        accounts: &[AccountInfo],
        instr_data: &[u8],
    ) -> ProgramResult {
        let instruction = PluginInstruction::try_from_slice(instr_data)
            .map_err(|_| ProgramError::InvalidInstructionData)?;
        msg!("Plugin: Deserialized instruction successfully.");

        match instruction {
            PluginInstruction::ComputeAddLiquidity {
                reserve_a,
                reserve_b,
                deposit_a,
                deposit_b,
                total_lp_supply,
            } => Self::compute_add_liquidity(
                accounts,
                reserve_a,
                reserve_b,
                deposit_a,
                deposit_b,
                total_lp_supply,
            ),
            PluginInstruction::ComputeRemoveLiquidity {
                reserve_a,
                reserve_b,
                total_lp_supply,
                lp_amount_burning,
            } => Self::compute_remove_liquidity(
                accounts,
                reserve_a,
                reserve_b,
                total_lp_supply,
                lp_amount_burning,
            ),
            PluginInstruction::ComputeSwap {
                reserve_in,
                reserve_out,
                amount_in,
            } => Self::compute_swap(accounts, reserve_in, reserve_out, amount_in),
        }
    }

    pub fn compute_add_liquidity(
        accounts: &[AccountInfo],
        reserve_a: u64,
        reserve_b: u64,
        deposit_a: u64,
        deposit_b: u64,
        total_lp_supply: u64,
    ) -> ProgramResult {
        // We store results in the first (and only) writable account => plugin state
        let state_acc = next_account_info(&mut accounts.iter())?;
        if !state_acc.is_writable {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut result = PluginCalcResult::default();
        msg!(
            "Plugin: Computing Add Liquidity. Reserves: ({}, {}), Deposit: ({}, {}), Total LP: {}",
            reserve_a,
            reserve_b,
            deposit_a,
            deposit_b,
            total_lp_supply
        );

        if total_lp_supply == 0 {
            // first deposit => geometric mean
            let prod = (deposit_a as u128).saturating_mul(deposit_b as u128);
            let minted = integer_sqrt(prod);
            if minted == 0 {
                return Err(ProgramError::InvalidArgument);
            }
            result.actual_a = deposit_a;
            result.actual_b = deposit_b;
            result.shares_to_mint = minted as u64;
        } else {
            // ratio-limited
            if reserve_a == 0 || reserve_b == 0 {
                return Err(ProgramError::InvalidArgument);
            }
            let req_b = (deposit_a as u128).saturating_mul(reserve_b as u128) / (reserve_a as u128);
            let req_a = (deposit_b as u128).saturating_mul(reserve_a as u128) / (reserve_b as u128);
            let mut actual_a = deposit_a;
            let mut actual_b = deposit_b;
            if req_b <= deposit_b as u128 {
                actual_b = req_b as u64;
            } else if req_a <= deposit_a as u128 {
                actual_a = req_a as u64;
            }
            // shares
            let shares_minted = (total_lp_supply as u128)
                .saturating_mul(actual_a as u128)
                .checked_div(reserve_a as u128)
                .unwrap_or(0);
            if shares_minted == 0 {
                return Err(ProgramError::InvalidArgument);
            }
            result.actual_a = actual_a;
            result.actual_b = actual_b;
            result.shares_to_mint = shares_minted as u64;
        }

        msg!(
            "Plugin: Calculated: actual_a={}, actual_b={}, shares={}",
            result.actual_a,
            result.actual_b,
            result.shares_to_mint
        );

        result.serialize(&mut *state_acc.data.borrow_mut())?;
        msg!("Plugin: Serialization successful.");

        Ok(())
    }

    pub fn compute_remove_liquidity(
        accounts: &[AccountInfo],
        reserve_a: u64,
        reserve_b: u64,
        total_lp_supply: u64,
        lp_amount_burning: u64,
    ) -> ProgramResult {
        let state_acc = next_account_info(&mut accounts.iter())?;
        if lp_amount_burning == 0 || lp_amount_burning > total_lp_supply {
            return Err(ProgramError::InvalidArgument);
        }
        let mut result = PluginCalcResult::default();

        // Standard floor division for withdrawals.
        // This strictly protects the pool and remaining LPs by leaving fractional
        // "dust" amounts in the pool.
        let w_a = (reserve_a as u128)
            .checked_mul(lp_amount_burning as u128)
            .and_then(|num| num.checked_div(total_lp_supply as u128))
            .unwrap_or(0);
        let w_b = (reserve_b as u128)
            .checked_mul(lp_amount_burning as u128)
            .and_then(|num| num.checked_div(total_lp_supply as u128))
            .unwrap_or(0);

        result.withdraw_a = w_a as u64;
        result.withdraw_b = w_b as u64;

        msg!(
            "Plugin RemoveLiquidity Calculated (Floor): withdraw_a={}, withdraw_b={}",
            result.withdraw_a,
            result.withdraw_b
        );

        result.serialize(&mut *state_acc.data.borrow_mut())?;
        Ok(())
    }

    pub fn compute_swap(
        accounts: &[AccountInfo],
        reserve_in: u64,
        reserve_out: u64,
        amount_in: u64,
    ) -> ProgramResult {
        let state_acc = next_account_info(&mut accounts.iter())?;
        if reserve_in == 0 || reserve_out == 0 || amount_in == 0 {
            // Allow amount_in = 0? Or return specific error?
            // For now, follow spl-token-swap pattern which seems to allow it
            // but results in 0 output.
            // Returning InvalidArgument if reserves are 0.
            if reserve_in == 0 || reserve_out == 0 {
                return Err(ProgramError::InvalidArgument);
            }
        }

        let mut result = PluginCalcResult::default();

        // Calculate effective input after 0.3% fee (floor division)
        let fee_num = 3u64;
        let fee_den = 1000u64;
        let effective_in = (amount_in as u128)
            .checked_mul(fee_den.saturating_sub(fee_num) as u128)
            .and_then(|num| num.checked_div(fee_den as u128))
            .unwrap_or(0);

        if effective_in == 0 && amount_in > 0 {
            // Fee took entire amount_in, result is 0 out
            result.amount_out = 0;
        } else {
            // Use spl-token-swap invariant-preserving logic with ceiling division
            let invariant = U192::from(reserve_in)
                .checked_mul(U192::from(reserve_out))
                .ok_or(ProgramError::InvalidInstructionData)?;

            let reserve_in_u128 = reserve_in as u128;
            let reserve_out_u128 = reserve_out as u128;

            let new_reserve_in_u128 = reserve_in_u128
                .checked_add(effective_in)
                .ok_or(ProgramError::InvalidInstructionData)?;

            // Need to downcast invariant safely before u128::checked_ceil_div
            let invariant_u128: u128 = invariant
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?;

            // Calculate minimum destination amount needed using ceiling division
            let (new_reserve_out_u128, _) = invariant_u128
                .checked_ceil_div(new_reserve_in_u128)
                .ok_or(ProgramError::InvalidInstructionData)?;

            // Calculate amount out based on the ceiling-derived new destination reserve
            let destination_amount_swapped_u128 = reserve_out_u128
                .checked_sub(new_reserve_out_u128)
                .ok_or(ProgramError::InvalidInstructionData)?;

            let amount_out: u64 = destination_amount_swapped_u128
                .try_into()
                .map_err(|_| ProgramError::InvalidInstructionData)?;

            result.amount_out = amount_out;
        }

        msg!(
            "Plugin Swap Calculated (CeilDiv Invariant): amount_out={}",
            result.amount_out
        );

        result.serialize(&mut *state_acc.data.borrow_mut())?;
        Ok(())
    }
}

fn integer_sqrt(v: u128) -> u128 {
    let mut x = v;
    let mut z = (v >> 1) + 1;
    while z < x {
        x = z;
        z = ((v / z) + z) >> 1;
    }
    x
}
