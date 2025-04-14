use crate::error::ContractError;
use cosmwasm_std::{DivideByZeroError, Isqrt, Uint128, Uint256};

/// Calculates the initial LP shares using the geometric mean: sqrt(a * b).
pub(crate) fn calculate_initial_lp_shares(
    amount_a: Uint128,
    amount_b: Uint128,
) -> Result<Uint128, ContractError> {
    if amount_a.is_zero() || amount_b.is_zero() {
        return Err(ContractError::ZeroInitialLiquidity {});
    }
    let prod = Uint256::from(amount_a) * Uint256::from(amount_b);
    let initial_lp_u256 = prod.isqrt();
    let initial_shares = Uint128::try_from(initial_lp_u256)?;
    if initial_shares.is_zero() {
        return Err(ContractError::InitialLiquidityTooLow {});
    }
    Ok(initial_shares)
}

/// Calculates LP shares for subsequent deposits based on the formula:
pub(crate) fn calculate_subsequent_lp_shares(
    amount_a: Uint128,
    amount_b: Uint128,
    reserve_a: Uint128,
    reserve_b: Uint128,
    total_shares: Uint128,
) -> Result<Uint128, ContractError> {
    if total_shares.is_zero() {
        return Err(ContractError::CalculateSharesWithZeroSupply {});
    }
    if reserve_a.is_zero() || reserve_b.is_zero() {
        return Err(ContractError::CalculateSharesWithZeroReserve {});
    }
    let share_a = amount_a.multiply_ratio(total_shares, reserve_a);
    let share_b = amount_b.multiply_ratio(total_shares, reserve_b);
    Ok(std::cmp::min(share_a, share_b))
}

/// Calculates the swap output amount using the constant product formula and applies fees.
pub(crate) fn calculate_swap_output(
    offer_amount: Uint128,
    reserve_in: Uint128,
    reserve_out: Uint128,
    fee_numerator: u64,
    fee_denominator: u64,
) -> Result<Uint128, ContractError> {
    if reserve_in.is_zero() || reserve_out.is_zero() {
        return Err(ContractError::SwapAgainstEmptyReserve {});
    }
    let reserve_in_plus_offer = reserve_in.checked_add(offer_amount)?;
    if reserve_in_plus_offer.is_zero() {
        return Err(DivideByZeroError {}.into());
    }
    let output_amount_before_fee = reserve_out.multiply_ratio(offer_amount, reserve_in_plus_offer);
    // TODO: Read fees from pool config or state if they become pool-specific
    let fee_amount = output_amount_before_fee
        .multiply_ratio(Uint128::from(fee_numerator), Uint128::from(fee_denominator));
    let output_amount = output_amount_before_fee.checked_sub(fee_amount)?;
    Ok(output_amount)
}

/// Calculates the amounts of token A and B to return for withdrawing a given amount of LP tokens.
pub(crate) fn calculate_withdraw_amounts(
    withdraw_lp_amount: Uint128,
    reserve_a: Uint128,
    reserve_b: Uint128,
    total_shares: Uint128,
) -> Result<(Uint128, Uint128), ContractError> {
    if total_shares.is_zero() {
        return Err(DivideByZeroError {}.into());
    }
    let return_a = reserve_a.multiply_ratio(withdraw_lp_amount, total_shares);
    let return_b = reserve_b.multiply_ratio(withdraw_lp_amount, total_shares);
    Ok((return_a, return_b))
}

#[cfg(test)]
mod tests {
    use super::*; // Import functions from parent module (calculations.rs)
    use crate::error::ContractError;
    use cosmwasm_std::{Isqrt, Uint128, Uint256};

    #[test]
    fn test_calculate_initial_lp_shares() {
        assert_eq!(
            calculate_initial_lp_shares(Uint128::new(100), Uint128::new(100)).unwrap(),
            Uint128::new(100)
        );
        assert_eq!(
            calculate_initial_lp_shares(Uint128::new(100), Uint128::new(400)).unwrap(),
            Uint128::new(200)
        );
        assert_eq!(
            calculate_initial_lp_shares(Uint128::new(1_000_000), Uint128::new(1_000_000)).unwrap(),
            Uint128::new(1_000_000)
        );
        // Test rounding
        let expected_sqrt_99 = (Uint256::from(99u128) * Uint256::from(1u128)).isqrt();
        assert_eq!(
            calculate_initial_lp_shares(Uint128::new(99), Uint128::new(1)).unwrap(),
            Uint128::try_from(expected_sqrt_99).unwrap()
        );
        // Test zero check
        let err_zero_a =
            calculate_initial_lp_shares(Uint128::zero(), Uint128::new(100)).unwrap_err();
        assert!(matches!(err_zero_a, ContractError::ZeroInitialLiquidity {}));
        let err_zero_b =
            calculate_initial_lp_shares(Uint128::new(100), Uint128::zero()).unwrap_err();
        assert!(matches!(err_zero_b, ContractError::ZeroInitialLiquidity {}));
    }

    #[test]
    fn test_calculate_subsequent_lp_shares() {
        let total_shares = Uint128::new(1000);
        let reserve_a = Uint128::new(100);
        let reserve_b = Uint128::new(200);
        // Proportional
        let shares = calculate_subsequent_lp_shares(
            Uint128::new(10),
            Uint128::new(20),
            reserve_a,
            reserve_b,
            total_shares,
        )
        .unwrap();
        assert_eq!(shares, Uint128::new(100));
        // Non-proportional
        let shares_non = calculate_subsequent_lp_shares(
            Uint128::new(10),
            Uint128::new(10),
            reserve_a,
            reserve_b,
            total_shares,
        )
        .unwrap();
        assert_eq!(shares_non, Uint128::new(50));
        // Zero reserves error
        let err_zero_res = calculate_subsequent_lp_shares(
            Uint128::new(10),
            Uint128::new(10),
            Uint128::zero(),
            reserve_b,
            total_shares,
        )
        .unwrap_err();
        assert!(matches!(
            err_zero_res,
            ContractError::CalculateSharesWithZeroReserve {}
        ));
        // Zero total shares error
        let err_zero_shares = calculate_subsequent_lp_shares(
            Uint128::new(10),
            Uint128::new(10),
            reserve_a,
            reserve_b,
            Uint128::zero(),
        )
        .unwrap_err();
        assert!(matches!(
            err_zero_shares,
            ContractError::CalculateSharesWithZeroSupply {}
        ));
    }

    #[test]
    fn test_calculate_swap_output() {
        let reserve_in = Uint128::new(1000);
        let reserve_out = Uint128::new(2000);
        let offer = Uint128::new(100);
        let fee_num = 3u64;
        let fee_den = 1000u64;
        let output =
            calculate_swap_output(offer, reserve_in, reserve_out, fee_num, fee_den).unwrap();
        assert_eq!(output, Uint128::new(181));
        // Large numbers
        let reserve_in_large = Uint128::new(1_000_000_000);
        let reserve_out_large = Uint128::new(2_000_000_000);
        let offer_large = Uint128::new(10_000_000);
        let output_large = calculate_swap_output(
            offer_large,
            reserve_in_large,
            reserve_out_large,
            fee_num,
            fee_den,
        )
        .unwrap();
        assert_eq!(output_large, Uint128::new(19_742_575));
        // Error zero reserves
        let err = calculate_swap_output(offer, Uint128::zero(), reserve_out, fee_num, fee_den)
            .unwrap_err();
        assert!(matches!(err, ContractError::SwapAgainstEmptyReserve {}));
    }

    #[test]
    fn test_calculate_withdraw_amounts() {
        let total_shares = Uint128::new(1000);
        let reserve_a = Uint128::new(100);
        let reserve_b = Uint128::new(200);
        let withdraw_lp = Uint128::new(100);
        let (a, b) =
            calculate_withdraw_amounts(withdraw_lp, reserve_a, reserve_b, total_shares).unwrap();
        assert_eq!(a, Uint128::new(10));
        assert_eq!(b, Uint128::new(20));
        // Error zero total shares
        let err = calculate_withdraw_amounts(withdraw_lp, reserve_a, reserve_b, Uint128::zero())
            .unwrap_err();
        assert!(matches!(err, ContractError::DivideByZeroError(..)));
    }
}
