#![deny(missing_docs)]
//! A basic pool program for swapping tokens.

/// Program entrypoint
pub mod entrypoint;
/// Custom program errors
pub mod error;
/// Instruction types
pub mod instruction;
/// Program derived address
pub mod pda;
/// Instruction processing logic
pub mod processor;
/// Program state
pub mod state;

// Export crate version
pub use solana_program;

// Expose the program ID constant
solana_program::declare_id!("DoPLd2CnrSxpcC1j13JvtS4XaoAehXkBMs61737M44Rq");
