// Device fingerprint — cached at boot, never PII.
//
// What's collected (and what each tells us):
//   • device_id: UUIDv4 generated at first launch, stored in
//     `settings.telemetry_device_id`. Stable across app restarts AND
//     login state changes. NOT a user identifier — it's tied to the
//     install. Two devices for the same user → two device_ids → useful
//     multi-device signal.
//   • app_version: Cargo.toml package version. Drives release-adoption
//     queries.
//   • os: `macos` / `win` / `linux` (the only three values the worker
//     accepts). Don't aggregate further.
//   • os_version: major only. macOS "15", Windows "11", Ubuntu "22.04"
//     style. Patch versions are noise.
//   • arch: `aarch64` / `x86_64`. Apple-silicon / Linux-ARM growth.
//   • locale: first 5 chars of system locale. Translation priorities.
//   • theme: current Settings theme.

use sqlx::SqlitePool;

use crate::shared::repos::settings as settings_repo;

const KEY_DEVICE_ID: &str = "telemetry_device_id";
const KEY_THEME: &str = "appearance"; // existing setting key in ZeroAny Workbench

#[derive(Debug, Clone)]
pub struct DeviceFingerprint {
    pub device_id: String,
    pub app_version: &'static str,
    pub os: &'static str,
    pub os_version: Option<String>,
    pub arch: &'static str,
    pub locale: Option<String>,
    pub theme: Option<String>,
}

/// Resolve the persistent device id, generating one on first call.
/// Always succeeds — falls back to an in-memory UUID if the DB write
/// fails (rare; we still want to be able to ping).
pub async fn ensure_device_id(pool: &SqlitePool) -> String {
    if let Ok(Some(row)) = settings_repo::get_by_key(pool, KEY_DEVICE_ID).await {
        if !row.value.is_empty() {
            return row.value;
        }
    }
    let new_id = uuid::Uuid::new_v4().to_string();
    // Best-effort write — if it fails, we'll regenerate next launch.
    // Telemetry rows would then bucket the same physical device under
    // two ids, but that's strictly worse than blocking startup.
    let _ = settings_repo::upsert(pool, KEY_DEVICE_ID, &new_id).await;
    new_id
}

pub async fn fingerprint(pool: &SqlitePool) -> DeviceFingerprint {
    DeviceFingerprint {
        device_id: ensure_device_id(pool).await,
        app_version: env!("CARGO_PKG_VERSION"),
        os: detect_os(),
        os_version: detect_os_version(),
        arch: detect_arch(),
        locale: detect_locale(),
        theme: detect_theme(pool).await,
    }
}

// ── Detection helpers ──────────────────────────────────────────────

fn detect_os() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "win"
    } else {
        // Anything else (Linux, BSD, etc.) groups under "linux" — the
        // worker's CHECK constraint only allows the three values, and
        // we don't ship to BSD anyway.
        "linux"
    }
}

fn detect_arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        // x86_64 is the only other arch we ship binaries for. 32-bit
        // and other arches fall through to this label — fine since the
        // worker only accepts the two values.
        "x86_64"
    }
}

fn detect_os_version() -> Option<String> {
    // Uses the `os_info` crate so the three OS families share one code
    // path:
    //   • macOS  → reads SystemVersion.plist  → "15.1.2" → trim to "15"
    //   • Windows→ reads the registry         → "10.0.22631" → "11"-ish
    //                                            (we keep the raw major)
    //   • Linux  → /etc/os-release VERSION_ID → "22.04" / "39" / etc.
    // We return the major component only — patch noise is useless for
    // product decisions, and the smaller cardinality keeps the index
    // tight.
    let info = os_info::get();
    let v = info.version();
    let s = v.to_string();
    if s == "Unknown" || s.is_empty() {
        return None;
    }
    // Strip everything after the first dot for macOS/Windows-style
    // dotted versions ("15.1.2" → "15"). Leave Ubuntu-style "22.04"
    // intact since the first segment ("22") loses the LTS distinction.
    // Heuristic: keep up to first dot UNLESS the second segment is
    // exactly two digits (Ubuntu/Debian style), in which case keep the
    // major.minor.
    if let Some(first_dot) = s.find('.') {
        let head = &s[..first_dot];
        let rest = &s[first_dot + 1..];
        let second = rest.split('.').next().unwrap_or("");
        if second.len() == 2 && second.chars().all(|c| c.is_ascii_digit()) {
            // Ubuntu / Debian style — keep "22.04"
            return Some(format!("{}.{}", head, second));
        }
        return Some(head.to_string());
    }
    Some(s)
}

fn detect_locale() -> Option<String> {
    // sys_locale's `get_locale` returns "en-US" / "de-DE" style strings
    // already trimmed to the BCP-47 shape we want. Falls back to None
    // on platforms where the lookup fails (uncommon).
    sys_locale::get_locale().map(|s| {
        // Cap at 12 chars — same as the worker's sanitiser.
        s.chars().take(12).collect()
    })
}

async fn detect_theme(pool: &SqlitePool) -> Option<String> {
    settings_repo::get_by_key(pool, KEY_THEME)
        .await
        .ok()
        .flatten()
        .map(|s| s.value)
}
