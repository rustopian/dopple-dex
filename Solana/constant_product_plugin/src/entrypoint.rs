use crate::processor::Processor;
use solana_program::{account_info::AccountInfo, entrypoint::ProgramResult, pubkey::Pubkey};
use solana_program::{entrypoint, msg};

entrypoint!(process_instruction);
pub fn process_instruction(
    program_id: &Pubkey,
    accounts: &[AccountInfo],
    instruction_data: &[u8],
) -> ProgramResult {
    msg!("Plugin Entrypoint: process_instruction called.");
    Processor::process(program_id, accounts, instruction_data)
}
