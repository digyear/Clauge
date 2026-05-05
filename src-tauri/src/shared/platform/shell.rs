//! User-shell abstraction.
//!
//! Different shells parse spawn-args completely differently:
//! - bash/zsh/fish: `-l` (login), `-i` (interactive), `-c "<cmd>"` (exec)
//! - PowerShell:    `-NoLogo`, `-NoProfile`, `-Command "<cmd>"`
//! - cmd.exe:       `/c "<cmd>"` (no login/interactive concepts)
//!
//! Centralising the per-shell args here means call sites only need to ask
//! "open a login shell that will run this command" without knowing how each
//! shell encodes that intent.


/// Identifies the shell family so we can pick correct spawn args.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Bash,
    Zsh,
    Fish,
    /// PowerShell 7+ (`pwsh.exe`) or Windows PowerShell 5 (`powershell.exe`).
    PowerShell,
    /// Legacy Windows command processor (`cmd.exe`).
    Cmd,
    /// Any shell we don't recognise — args are passed bash-style on Unix
    /// and PowerShell-style on Windows as a best guess.
    Unknown,
}

impl ShellKind {
    /// Detect from a binary path (e.g. `/bin/zsh`, `C:\Windows\System32\cmd.exe`).
    /// Splits on both `/` and `\` so a Windows path parses correctly on a
    /// Unix host (and vice versa) — Rust's `Path` uses native separators only.
    pub fn detect(path: &str) -> Self {
        let basename = path
            .rsplit(|c| c == '/' || c == '\\')
            .next()
            .unwrap_or(path);
        let stem = basename
            .rsplit_once('.')
            .map(|(stem, _ext)| stem)
            .unwrap_or(basename)
            .to_lowercase();
        match stem.as_str() {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            "pwsh" | "powershell" => Self::PowerShell,
            "cmd" => Self::Cmd,
            _ => Self::Unknown,
        }
    }

    /// Args for a login + interactive session that sources rc files.
    /// Used when opening a plain shell terminal (no command to run).
    pub fn interactive_login_args(&self) -> Vec<&'static str> {
        match self {
            Self::Bash | Self::Zsh | Self::Fish => vec!["-l", "-i"],
            // PowerShell loads its profile by default; suppress the banner only.
            Self::PowerShell => vec!["-NoLogo"],
            Self::Cmd => vec![],
            // Best-effort: assume Unix-y on non-Windows, PowerShell-y on Windows.
            Self::Unknown if cfg!(target_os = "windows") => vec!["-NoLogo"],
            Self::Unknown => vec!["-l", "-i"],
        }
    }

    /// Build full argv for "open a login interactive shell, then exec this single command".
    /// The caller passes `cmd` as a single string (already shell-escaped where needed).
    pub fn exec_command_argv(&self, cmd: &str) -> Vec<String> {
        match self {
            Self::Bash | Self::Zsh | Self::Fish => {
                vec!["-l".into(), "-i".into(), "-c".into(), cmd.to_string()]
            }
            Self::PowerShell => {
                vec!["-NoLogo".into(), "-Command".into(), cmd.to_string()]
            }
            Self::Cmd => vec!["/c".into(), cmd.to_string()],
            Self::Unknown if cfg!(target_os = "windows") => {
                vec!["-NoLogo".into(), "-Command".into(), cmd.to_string()]
            }
            Self::Unknown => {
                vec!["-l".into(), "-i".into(), "-c".into(), cmd.to_string()]
            }
        }
    }

}

/// Returns the user's preferred interactive shell as (binary_path, kind).
///
/// On Unix:    `$SHELL`, falling back to `/bin/zsh`.
/// On Windows: prefers `pwsh.exe` if `POWERSHELL_DISTRIBUTION_CHANNEL` is
///             set (PowerShell 7 auto-sets this); otherwise `%COMSPEC%`
///             (typically `cmd.exe`); falls back to `powershell.exe`.
///
/// Why prefer pwsh on Windows: it's modern, cross-platform, and what most
/// developer-tool users have installed. cmd.exe has weak PTY semantics and
/// no scripting parity with bash.
pub fn default_user_shell() -> (String, ShellKind) {
    let path = if cfg!(target_os = "windows") {
        if std::env::var("POWERSHELL_DISTRIBUTION_CHANNEL").is_ok() {
            "pwsh.exe".to_string()
        } else {
            std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
        }
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| {
            // zsh is macOS default but not shipped on Ubuntu/Debian.
            // Prefer bash (universal) then fall back to POSIX sh.
            if std::path::Path::new("/bin/bash").exists() {
                "/bin/bash".to_string()
            } else {
                "/bin/sh".to_string()
            }
        })
    };
    let kind = ShellKind::detect(&path);
    (path, kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_unix_shells() {
        assert_eq!(ShellKind::detect("/bin/bash"), ShellKind::Bash);
        assert_eq!(ShellKind::detect("/usr/local/bin/zsh"), ShellKind::Zsh);
        assert_eq!(ShellKind::detect("/opt/homebrew/bin/fish"), ShellKind::Fish);
    }

    #[test]
    fn detects_windows_shells() {
        assert_eq!(
            ShellKind::detect(r"C:\Program Files\PowerShell\7\pwsh.exe"),
            ShellKind::PowerShell
        );
        assert_eq!(
            ShellKind::detect(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe"),
            ShellKind::PowerShell
        );
        assert_eq!(
            ShellKind::detect(r"C:\Windows\System32\cmd.exe"),
            ShellKind::Cmd
        );
    }

    #[test]
    fn unknown_shell_falls_through() {
        assert_eq!(ShellKind::detect("/usr/local/bin/nu"), ShellKind::Unknown);
    }

    #[test]
    fn bash_exec_argv() {
        let argv = ShellKind::Bash.exec_command_argv("echo hi");
        assert_eq!(argv, vec!["-l", "-i", "-c", "echo hi"]);
    }

    #[test]
    fn powershell_exec_argv() {
        let argv = ShellKind::PowerShell.exec_command_argv("Get-Process");
        assert_eq!(argv, vec!["-NoLogo", "-Command", "Get-Process"]);
    }

    #[test]
    fn cmd_exec_argv() {
        let argv = ShellKind::Cmd.exec_command_argv("dir");
        assert_eq!(argv, vec!["/c", "dir"]);
    }

    #[test]
    fn login_args_per_shell() {
        assert_eq!(ShellKind::Zsh.interactive_login_args(), vec!["-l", "-i"]);
        assert_eq!(
            ShellKind::PowerShell.interactive_login_args(),
            vec!["-NoLogo"]
        );
        assert_eq!(
            ShellKind::Cmd.interactive_login_args(),
            Vec::<&'static str>::new()
        );
    }
}
