use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Event, Uint128};

// Events specific to this pool contract

#[cw_serde]
pub struct LiquidityAddedEvent {
    pub sender: Addr,
    pub denom_a_deposited: Uint128,
    pub denom_b_deposited: Uint128,
    pub shares_minted: Uint128,
}

impl From<LiquidityAddedEvent> for Event {
    fn from(val: LiquidityAddedEvent) -> Self {
        Event::new("liquidity_added")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("denom_a_deposited", val.denom_a_deposited.to_string())
            .add_attribute("denom_b_deposited", val.denom_b_deposited.to_string())
            .add_attribute("shares_minted", val.shares_minted.to_string())
    }
}

#[cw_serde]
pub struct LiquidityRemovedEvent {
    pub sender: Addr,            // User receiving funds
    pub lp_token_contract: Addr, // LP token contract that was burned from
    pub withdrawn_share: Uint128,
    pub return_a: Uint128,
    pub return_b: Uint128,
}

impl From<LiquidityRemovedEvent> for Event {
    fn from(val: LiquidityRemovedEvent) -> Self {
        Event::new("liquidity_removed")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("lp_token_contract", val.lp_token_contract.into_string())
            .add_attribute("withdrawn_share", val.withdrawn_share.to_string())
            .add_attribute("return_a", val.return_a.to_string())
            .add_attribute("return_b", val.return_b.to_string())
    }
}

#[cw_serde]
pub struct SwapEvent {
    pub sender: Addr,
    pub offer_denom: String,
    pub ask_denom: String,
    pub offer_amount: Uint128,
    pub return_amount: Uint128,
}

impl From<SwapEvent> for Event {
    fn from(val: SwapEvent) -> Self {
        Event::new("swap")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("offer_denom", val.offer_denom)
            .add_attribute("ask_denom", val.ask_denom)
            .add_attribute("offer_amount", val.offer_amount.to_string())
            .add_attribute("return_amount", val.return_amount.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::{Addr, Event, Uint128};

    #[test]
    fn test_event_conversion() {
        let addr1 = Addr::unchecked("addr1");
        let addr2 = Addr::unchecked("addr2");

        let added = LiquidityAddedEvent {
            sender: addr1.clone(),
            denom_a_deposited: Uint128::new(50),
            denom_b_deposited: Uint128::new(100),
            shares_minted: Uint128::new(70),
        };
        let event: Event = added.into();
        assert_eq!(event.ty, "liquidity_added");
        assert!(event.attributes.contains(&("shares_minted", "70").into()));
        assert!(event
            .attributes
            .contains(&("denom_a_deposited", "50").into()));

        let removed = LiquidityRemovedEvent {
            sender: addr1.clone(),
            lp_token_contract: addr2.clone(),
            withdrawn_share: Uint128::new(100),
            return_a: Uint128::new(50),
            return_b: Uint128::new(100),
        };
        let event: Event = removed.into();
        assert_eq!(event.ty, "liquidity_removed");
        assert!(event.attributes.contains(&("return_a", "50").into()));
        assert!(event
            .attributes
            .contains(&("lp_token_contract", "addr2").into()));
    }
}
