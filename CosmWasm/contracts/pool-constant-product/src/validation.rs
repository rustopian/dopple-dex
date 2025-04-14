use crate::error::ContractError;
use cosmwasm_std::{Decimal, MessageInfo, Uint128};

/// Validates that the MessageInfo contains funds for the two pool denoms and returns the amounts.
/// Errors if funds are missing, zero, or contain invalid denoms.
pub(crate) fn validate_and_get_liquidity_funds(
    info: &MessageInfo,
    pool_denom_a: &str,
    pool_denom_b: &str,
) -> Result<(Uint128, Uint128), ContractError> {
    let mut amount_a = Uint128::zero();
    let mut amount_b = Uint128::zero();
    for coin in info.funds.iter() {
        if coin.denom == pool_denom_a {
            amount_a = coin.amount;
        } else if coin.denom == pool_denom_b {
            amount_b = coin.amount;
        } else {
            return Err(ContractError::InvalidLiquidityDenom {
                denom: coin.denom.clone(),
            });
        }
    }
    if amount_a.is_zero() || amount_b.is_zero() {
        return Err(ContractError::MissingLiquidityToken {});
    }
    Ok((amount_a, amount_b))
}

/// Extracts the amount of the specified offer_denom from the MessageInfo funds.
/// Errors if the offer_denom is not found or the amount is zero.
pub(crate) fn get_offer_amount(
    info: &MessageInfo,
    offer_denom: &str,
) -> Result<Uint128, ContractError> {
    let offer_coin = info
        .funds
        .iter()
        .find(|c| c.denom == offer_denom)
        .ok_or_else(|| ContractError::NoMatchingOfferCoin {
            denom: offer_denom.to_string(),
        })?;
    if offer_coin.amount.is_zero() {
        return Err(ContractError::ZeroOfferAmount {});
    }
    Ok(offer_coin.amount)
}

/// Validates if the ratio of deposited amounts matches the reserve ratio within 1% slippage.
pub(crate) fn validate_deposit_ratio(
    amount_a: Uint128,
    amount_b: Uint128,
    reserve_a: Uint128,
    reserve_b: Uint128,
) -> Result<(), ContractError> {
    if reserve_a.is_zero() || reserve_b.is_zero() {
        return Err(ContractError::ValidateRatioWithZeroReserve {});
    }
    // TODO: Make slippage configurable?
    let slippage = Decimal::percent(1);
    let ratio_a = Decimal::from_ratio(amount_a, reserve_a);
    let ratio_b = Decimal::from_ratio(amount_b, reserve_b);
    let diff = if ratio_a > ratio_b {
        ratio_a - ratio_b
    } else {
        ratio_b - ratio_a
    };
    if diff > slippage {
        return Err(ContractError::DepositRatioMismatch {});
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ContractError;
    use cosmwasm_std::testing::message_info;
    use cosmwasm_std::{coin, Addr, Uint128};

    const USER1: &str = "user_address_111";
    const DENOM_A: &str = "token_a";
    const DENOM_B: &str = "token_b";

    #[test]
    fn test_validate_and_get_liquidity_funds() {
        let info_ok = message_info(
            &Addr::unchecked(USER1),
            &[coin(100, DENOM_A), coin(200, DENOM_B)],
        );
        let (a, b) = validate_and_get_liquidity_funds(&info_ok, DENOM_A, DENOM_B).unwrap();
        assert_eq!(a, Uint128::new(100));
        assert_eq!(b, Uint128::new(200));
        let info_zero_a = message_info(
            &Addr::unchecked(USER1),
            &[coin(0, DENOM_A), coin(200, DENOM_B)],
        );
        let err_zero_a =
            validate_and_get_liquidity_funds(&info_zero_a, DENOM_A, DENOM_B).unwrap_err();
        assert!(matches!(
            err_zero_a,
            ContractError::MissingLiquidityToken {}
        ));
        let info_zero_b = message_info(
            &Addr::unchecked(USER1),
            &[coin(100, DENOM_A), coin(0, DENOM_B)],
        );
        let err_zero_b =
            validate_and_get_liquidity_funds(&info_zero_b, DENOM_A, DENOM_B).unwrap_err();
        assert!(matches!(
            err_zero_b,
            ContractError::MissingLiquidityToken {}
        ));
        let info_missing = message_info(&Addr::unchecked(USER1), &[coin(100, DENOM_A)]);
        let err_missing =
            validate_and_get_liquidity_funds(&info_missing, DENOM_A, DENOM_B).unwrap_err();
        assert!(matches!(
            err_missing,
            ContractError::MissingLiquidityToken {}
        ));
        let info_invalid = message_info(
            &Addr::unchecked(USER1),
            &[coin(100, DENOM_A), coin(200, "bad_denom")],
        );
        let err_invalid =
            validate_and_get_liquidity_funds(&info_invalid, DENOM_A, DENOM_B).unwrap_err();
        assert!(
            matches!(err_invalid, ContractError::InvalidLiquidityDenom { denom } if denom == "bad_denom")
        );
    }

    #[test]
    fn test_get_offer_amount() {
        let info_ok = message_info(
            &Addr::unchecked(USER1),
            &[coin(100, DENOM_A), coin(200, DENOM_B)],
        );
        assert_eq!(
            get_offer_amount(&info_ok, DENOM_A).unwrap(),
            Uint128::new(100)
        );
        assert_eq!(
            get_offer_amount(&info_ok, DENOM_B).unwrap(),
            Uint128::new(200)
        );
        let err_not_found = get_offer_amount(&info_ok, "tokenC").unwrap_err();
        assert!(
            matches!(err_not_found, ContractError::NoMatchingOfferCoin { denom } if denom == "tokenC")
        );
        let info_zero = message_info(&Addr::unchecked(USER1), &[coin(0, DENOM_A)]);
        let err_zero = get_offer_amount(&info_zero, DENOM_A).unwrap_err();
        assert!(matches!(err_zero, ContractError::ZeroOfferAmount {}));
    }

    #[test]
    fn test_validate_deposit_ratio() {
        let reserve_a = Uint128::new(1000);
        let reserve_b = Uint128::new(2000);
        assert!(
            validate_deposit_ratio(Uint128::new(10), Uint128::new(20), reserve_a, reserve_b)
                .is_ok()
        );
        assert!(validate_deposit_ratio(
            Uint128::new(100),
            Uint128::from(201u128),
            reserve_a,
            reserve_b
        )
        .is_ok());
        let err =
            validate_deposit_ratio(Uint128::new(100), Uint128::new(250), reserve_a, reserve_b)
                .unwrap_err();
        assert!(matches!(err, ContractError::DepositRatioMismatch {}));
        let err_zero = validate_deposit_ratio(
            Uint128::new(10),
            Uint128::new(20),
            Uint128::zero(),
            reserve_b,
        )
        .unwrap_err();
        assert!(matches!(
            err_zero,
            ContractError::ValidateRatioWithZeroReserve {}
        ));
    }
}
