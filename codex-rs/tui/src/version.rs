/// The current Codex CLI version as embedded at compile time.
#[cfg(not(test))]
pub const CODEX_CLI_VERSION: &str = env!("CARGO_PKG_VERSION");

// Keep UI fixtures independent of the alpha version assigned during packaging.
#[cfg(test)]
pub const CODEX_CLI_VERSION: &str = "0.0.0";
