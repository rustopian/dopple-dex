pub mod instruction;
pub mod processor;

#[cfg(not(feature = "no-entrypoint"))]
pub mod entrypoint;

// Export crate version
pub use solana_program;

#[cfg(test)]
mod processor_tests;
