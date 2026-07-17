// Resolve a `&'static dyn CliRunner` from a provider id string. The
// string is what we persist on `agent_sessions.provider` and what the
// frontend hands us when creating / resuming a session. Unknown ids
// fall back to Claude — every existing row pre-migration-13 was
// implicitly Claude, so this keeps legacy / typo-bound rows working
// instead of failing loudly. New code paths that genuinely need to
// reject unknown providers should call `try_runner_for` instead.

use super::claude::CLAUDE;
use super::codex::CODEX;
use super::gemini::GEMINI;
use super::hermes::HERMES;
use super::opencode::OPENCODE;
use super::runner::CliRunner;

/// Map a provider id to its runner. Unknown ids → Claude (safe default).
pub fn runner_for(provider: &str) -> &'static dyn CliRunner {
    match provider {
        "codex" => &CODEX,
        "gemini" => &GEMINI,
        "hermes" => &HERMES,
        "opencode" => &OPENCODE,
        _ => &CLAUDE,
    }
}

/// Strict variant: returns None for unknown provider ids. Use when the
/// caller cares about typos / new providers slipping past Claude's
/// default catch-all.
pub fn try_runner_for(provider: &str) -> Option<&'static dyn CliRunner> {
    match provider {
        "claude" => Some(&CLAUDE),
        "codex" => Some(&CODEX),
        "gemini" => Some(&GEMINI),
        "hermes" => Some(&HERMES),
        "opencode" => Some(&OPENCODE),
        _ => None,
    }
}

/// All providers Clauge currently supports, in display order. Drives
/// the provider tab strip in the plugin manager and the picker in
/// NewSessionModal. Keep Claude first so it stays the obvious default.
pub const SUPPORTED_PROVIDERS: &[&str] = &["claude", "codex", "gemini", "opencode", "hermes"];
