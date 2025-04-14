use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Addr, Event, Uint128};

// --- Event Structs ---

#[cw_serde]
pub struct PoolCreatedEvent {
    pub denom_a: String,
    pub denom_b: String,
    pub lp_token_addr: Addr,
    pub lp_token_code_id: u64,
}

impl From<PoolCreatedEvent> for Event {
    fn from(val: PoolCreatedEvent) -> Self {
        Event::new("pool_created")
            .add_attribute("denom_a", val.denom_a)
            .add_attribute("denom_b", val.denom_b)
            .add_attribute("lp_token_addr", val.lp_token_addr.into_string())
            .add_attribute("lp_token_code_id", val.lp_token_code_id.to_string())
    }
}

#[cw_serde]
pub struct InitialLiquidityProvidedEvent {
    pub sender: Addr,
    pub denom_a: String,
    pub denom_b: String,
    pub amount_a: Uint128,
    pub amount_b: Uint128,
    pub initial_shares: Uint128,
}

impl From<InitialLiquidityProvidedEvent> for Event {
    fn from(val: InitialLiquidityProvidedEvent) -> Self {
        Event::new("initial_liquidity_provided")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("denom_a", val.denom_a)
            .add_attribute("denom_b", val.denom_b)
            .add_attribute("amount_a", val.amount_a.to_string())
            .add_attribute("amount_b", val.amount_b.to_string())
            .add_attribute("initial_shares", val.initial_shares.to_string())
    }
}

#[cw_serde]
pub struct LiquidityAddedEvent {
    pub sender: Addr,
    pub denom_a: String,
    pub denom_b: String,
    pub amount_a: Uint128,
    pub amount_b: Uint128,
    pub shares_minted: Uint128,
}

impl From<LiquidityAddedEvent> for Event {
    fn from(val: LiquidityAddedEvent) -> Self {
        Event::new("liquidity_added")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("denom_a", val.denom_a)
            .add_attribute("denom_b", val.denom_b)
            .add_attribute("amount_a", val.amount_a.to_string())
            .add_attribute("amount_b", val.amount_b.to_string())
            .add_attribute("shares_minted", val.shares_minted.to_string())
    }
}

#[cw_serde]
pub struct LiquidityRemovedEvent {
    pub sender: Addr,
    pub lp_token_sender: Addr,
    pub denom_a: String,
    pub denom_b: String,
    pub withdrawn_share: Uint128,
    pub return_a: Uint128,
    pub return_b: Uint128,
}

impl From<LiquidityRemovedEvent> for Event {
    fn from(val: LiquidityRemovedEvent) -> Self {
        Event::new("liquidity_removed")
            .add_attribute("sender", val.sender.into_string())
            .add_attribute("lp_token_sender", val.lp_token_sender.into_string())
            .add_attribute("denom_a", val.denom_a)
            .add_attribute("denom_b", val.denom_b)
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
    use cosmwasm_std::Addr;

    #[test]
    fn test_event_conversion() {
        let addr1 = Addr::unchecked("addr1");
        let addr2 = Addr::unchecked("addr2");

        let created = PoolCreatedEvent {
            denom_a: "a".to_string(),
            denom_b: "b".to_string(),
            lp_token_addr: addr1.clone(),
            lp_token_code_id: 1,
        };
        let _event: Event = created.into();
        let initial = InitialLiquidityProvidedEvent {
            sender: addr1.clone(),
            denom_a: "a".to_string(),
            denom_b: "b".to_string(),
            amount_a: Uint128::new(100),
            amount_b: Uint128::new(200),
            initial_shares: Uint128::new(141),
        };
        let _event: Event = initial.into();
        let added = LiquidityAddedEvent {
            sender: addr1.clone(),
            denom_a: "a".to_string(),
            denom_b: "b".to_string(),
            amount_a: Uint128::new(50),
            amount_b: Uint128::new(100),
            shares_minted: Uint128::new(70),
        };
        let _event: Event = added.into();
        let removed = LiquidityRemovedEvent {
            sender: addr1.clone(),
            lp_token_sender: addr2.clone(),
            denom_a: "a".to_string(),
            denom_b: "b".to_string(),
            withdrawn_share: Uint128::new(100),
            return_a: Uint128::new(50),
            return_b: Uint128::new(100),
        };
        let _event: Event = removed.into();
        let swap = SwapEvent {
            sender: addr1.clone(),
            offer_denom: "a".to_string(),
            ask_denom: "b".to_string(),
            offer_amount: Uint128::new(100),
            return_amount: Uint128::new(180),
        };
        let _event: Event = swap.into();
    }
}
