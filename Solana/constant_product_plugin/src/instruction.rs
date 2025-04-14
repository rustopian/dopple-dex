use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshSerialize, BorshDeserialize, Debug)]
pub enum PluginInstruction {
    ComputeAddLiquidity {
        reserve_a: u64,
        reserve_b: u64,
        deposit_a: u64,
        deposit_b: u64,
        total_lp_supply: u64,
    },
    ComputeRemoveLiquidity {
        reserve_a: u64,
        reserve_b: u64,
        total_lp_supply: u64,
        lp_amount_burning: u64,
    },
    ComputeSwap {
        reserve_in: u64,
        reserve_out: u64,
        amount_in: u64,
    },
}
