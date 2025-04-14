#![deny(missing_docs)]
//! A basic pool program for swapping tokens.

/// Program entrypoint
pub mod entrypoint;
/// Custom program errors
pub mod error;
/// Instruction types
pub mod instruction;
/// Instruction processing logic
pub mod processor;
/// Program state
pub mod state;
/// Program derived address
pub mod pda;

// Export crate version
pub use solana_program;

#[cfg(test)]
mod processor_tests;

// Expose the program ID constant
solana_program::declare_id!("DoPLd2CnrSxpcC1j13JvtS4XaoAehXkBMs61737M44Rq");
