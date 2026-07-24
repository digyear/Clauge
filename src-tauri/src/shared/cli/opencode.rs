// OpenCode CLI implementation of [`CliRunner`].
//
// OpenCode (opencode.ai) is fundamentally a different shape from
// Claude / Codex — worth flagging upfront so the gaps are intentional:
//   - Non-interactive form is `opencode run <message>` (with `-c` to
//     continue last session, `-s <id>` to continue a specific one).
//     Interactive is the bare `opencode` invocation (TUI by default).
//   - System-prompt injection has no first-class flag. OpenCode's
//     personality comes from `opencode agent create` (a separate config
//     concept) or from per-project `AGENTS.md`. For coworker drawer
//     turns the persona is prepended into the message itself — handled
//     by the oneshot_argv caller, not by build_spawn_command.
//   - Plugins are **npm packages**, not marketplace directories.
//     `opencode plugin <module>` installs into `~/.config/opencode/`
//     as a regular npm dep. There is no "marketplace" concept, so the
//     plugin-marketplace UI doesn't apply to this provider. We surface
//     this by returning `None` from the marketplace + install-counts
//     hooks; the frontend's plugin tab strip hides itself for OpenCode.
//   - Sessions live in a single SQLite DB at
//     `~/.local/share/opencode/opencode.db`, not as per-project files.
//     Per-project session discovery is therefore skipped for v1.
//   - MCP config lives in `opencode.json` (per-project) or
//     `~/.config/opencode/opencode.json` (global), under an `mcp` key
//     with `type: 'remote'` or `type: 'local'` entries. The workspace
//     MCP registration writes that file directly rather than going
//     through `opencode mcp add`, which is interactive-only.

use std::path::{Path, PathBuf};

use super::runner::{CliRunner, SpawnOpts};

pub struct OpenCodeRunner;

const BINARY: &str = "opencode";
const SESSION_EXT: &str = "jsonl";

impl OpenCodeRunner {
    /// `~/.config/opencode` — XDG-style config home.
    fn config_dir(&self) -> Option<PathBuf> {
        // Honour XDG_CONFIG_HOME when set; fall back to `~/.config`.
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.trim().is_empty() {
                return Some(PathBuf::from(xdg).join("opencode"));
            }
        }
        dirs::home_dir().map(|h| h.join(".config").join("opencode"))
    }

    /// `~/.local/share/opencode` — XDG-style data home, where the session
    /// SQLite DB and logs live.
    fn data_dir(&self) -> Option<PathBuf> {
        if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
            if !xdg.trim().is_empty() {
                return Some(PathBuf::from(xdg).join("opencode"));
            }
        }
        dirs::home_dir().map(|h| h.join(".local").join("share").join("opencode"))
    }
}

impl CliRunner for OpenCodeRunner {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn binary_name(&self) -> &'static str {
        BINARY
    }

    fn resolve_binary_path(&self) -> String {
        crate::shared::platform::path::find_binary(BINARY)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| BINARY.to_string())
    }

    fn build_spawn_command(&self, opts: &SpawnOpts) -> String {
        let head = opts.binary_path_override.as_deref()
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .map(crate::shared::cli::runner::shell_quote_path)
            .unwrap_or_else(|| BINARY.to_string());
        // Bare `opencode` launches the TUI. `-s <id>` resumes a specific
        // session; `-c` would continue the last. OpenCode session ids
        // are `ses_<base62>` — anything else (e.g. a Codex UUID that
        // leaked through from a stale frontend session row) would make
        // `opencode -s` reject the spawn with "Expected ses, got …".
        // Drop the resume arg silently in that case and start fresh
        // rather than crashing the terminal.
        let mut cmd = head;
        if let Some(ref sid) = opts.resume_session_id {
            if sid.starts_with("ses_") {
                cmd.push_str(&format!(" -s \"{}\"", sid));
            }
        }
        if opts.skip_permissions {
            // OpenCode mirrors Claude's flag name verbatim.
            cmd.push_str(" --dangerously-skip-permissions");
        }
        // OpenCode has no free-form `--system-prompt` analogue. The
        // session's purpose / Custom prompt is instead written into
        // `AGENTS.md` at the project root pre-spawn (mirroring the
        // Gemini path) — see `agent_inject_purpose` in
        // modes/agent/commands.rs and its trigger in AgentPanel. We
        // consume the field here so the spawn opts stay uniform
        // across runners; the actual prompt delivery happens via the
        // file write before this command runs.
        let _ = opts.system_prompt;
        cmd
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.config_dir()
    }

    fn plugins_dir(&self) -> Option<PathBuf> {
        // OpenCode plugins are npm deps under the config dir; the
        // node_modules folder is their effective install root.
        self.config_dir().map(|p| p.join("node_modules"))
    }

    fn settings_file(&self) -> Option<PathBuf> {
        // Primary config; per-project `opencode.json` files override it.
        self.config_dir().map(|p| p.join("opencode.json"))
    }

    fn installed_plugins_file(&self) -> Option<PathBuf> {
        // No standalone installed_plugins.json — the list is the
        // `dependencies` block in `~/.config/opencode/package.json`.
        // Returning None tells the frontend plugin manager to hide the
        // tab for OpenCode (npm package management is a different UX).
        None
    }

    fn plugin_marketplaces_dir(&self) -> Option<PathBuf> {
        // No marketplace concept; npm is the registry.
        None
    }

    fn plugin_install_counts_file(&self) -> Option<PathBuf> {
        None
    }

    fn run_plugin_subcommand(&self, _args: &[&str]) -> Result<(bool, String), String> {
        // `opencode plugin <module>` is an install command, not a listing
        // surface. The ZeroAny Workbench plugin manager doesn't apply to OpenCode.
        Err("OpenCode plugins are npm packages; the plugin manager doesn't apply here.".into())
    }

    fn sessions_root(&self) -> Option<PathBuf> {
        self.data_dir()
    }

    fn session_dir_for_project(&self, _project_path: &str) -> Option<PathBuf> {
        // Sessions are rows in `opencode.db`, not files in a directory.
        // Per-project discovery would need a SQL query against the
        // database; deferred until users ask for it.
        None
    }

    fn session_file_extension(&self) -> &'static str {
        SESSION_EXT
    }

    fn extract_resume_id_from_output(&self, _buffer: &str) -> Option<String> {
        // OpenCode session ids look like `ses_<base62>` and surface
        // through `opencode session list` or the TUI header. PTY-based
        // capture isn't deterministic; the caller polls the sessions DB
        // after spawn instead.
        None
    }

    fn usage_api_orgs_url(&self) -> Option<String> {
        // No first-party usage analytics endpoint. `opencode stats` is the
        // local source of truth; not wired up server-side yet.
        None
    }

    fn usage_api_url_for(&self, _org_id: &str) -> Option<String> {
        None
    }

    fn is_session_file(&self, _path: &Path) -> bool {
        false
    }
}

pub static OPENCODE: OpenCodeRunner = OpenCodeRunner;
