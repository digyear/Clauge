// CLI-runner abstraction: the contract Agent mode (and any future mode that
// wraps a coding-assistant CLI) relies on. See `runner.rs` for the trait,
// `claude.rs` for the Claude implementation.

pub mod claude;
pub mod runner;
