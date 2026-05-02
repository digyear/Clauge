//! Tauri commands for channel-aware update check + install.

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::shared::updater::channel::resolve_endpoint;
use crate::shared::updater::state::PendingUpdate;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub body: String,
}

/// Resolve the channel endpoint and call the Tauri updater's `check()`.
///
/// Shared by both the auto-install path (`check_for_update_in_channel`) and
/// the info-only path (`check_latest_version`) so channel resolution and
/// signature verification stay in one place.
async fn run_update_check(
    app: &AppHandle,
    pool: &SqlitePool,
    channel: &str,
) -> Result<Option<Update>, String> {
    let endpoint = resolve_endpoint(pool, channel).await?;
    let endpoint_url =
        url::Url::parse(&endpoint).map_err(|e| format!("invalid updater URL: {}", e))?;

    app.updater_builder()
        .endpoints(vec![endpoint_url])
        .map_err(|e| format!("updater endpoints: {}", e))?
        .build()
        .map_err(|e| format!("updater build: {}", e))?
        .check()
        .await
        .map_err(|e| format!("update check: {}", e))
}

/// Frontend entry-point for installable platforms (macOS / Windows / AppImage).
/// Resolves the right endpoint for the given channel, runs `check()`, and
/// if there's an update, downloads it and stashes the `Update` in state for
/// a follow-up `install_pending_update` call.
///
/// Returns metadata for the new version, or `None` if already up to date.
#[tauri::command]
pub async fn check_for_update_in_channel(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    pending: State<'_, PendingUpdate>,
    channel: String,
) -> Result<Option<UpdateInfo>, String> {
    let Some(update) = run_update_check(&app, pool.inner(), &channel).await? else {
        return Ok(None);
    };

    // Pre-download so the install step is fast and offline-tolerant. We
    // stash both the Update *and* the downloaded bytes in state — the next
    // call (install) needs the bytes back. `Update::install(bytes)` does
    // not re-fetch on its own (the Rust Update has no internal byte cache,
    // unlike the JS plugin's stateful Update object); passing an empty Vec
    // makes `extract()` fail on every platform with no user feedback.
    let info = UpdateInfo {
        version: update.version.clone(),
        body: update.body.clone().unwrap_or_default(),
    };

    let bytes = update
        .download(|_chunk_len, _content_len| {}, || {})
        .await
        .map_err(|e| format!("update download: {}", e))?;

    pending.store(update, bytes);
    Ok(Some(info))
}

/// Info-only check for platforms where Tauri's auto-install can't run
/// (currently Linux .deb / .rpm — owned by the system package manager).
///
/// Hits the same signed `latest.json` channel as the auto-install path but
/// stops short of `download()`/`install()`. The frontend uses the returned
/// version + release notes to render a "new version available, download
/// from releases page" toast — no binary is fetched, no Pending state is
/// touched. Returns `None` if up-to-date.
#[tauri::command]
pub async fn check_latest_version(
    app: AppHandle,
    pool: State<'_, SqlitePool>,
    channel: String,
) -> Result<Option<UpdateInfo>, String> {
    let Some(update) = run_update_check(&app, pool.inner(), &channel).await? else {
        return Ok(None);
    };
    Ok(Some(UpdateInfo {
        version: update.version.clone(),
        body: update.body.clone().unwrap_or_default(),
    }))
}

/// Install the most recently downloaded update and hand off to the OS.
///
/// This call does not return on success — the process either exits (Windows)
/// or is replaced by `app.restart()` (macOS / Linux AppImage). The frontend
/// must NOT call `tauri-plugin-process`'s `relaunch()` after this; doing so
/// races the in-progress installer (was the cause of "Restart stuck" on
/// Windows — `app.restart` would `CreateProcess(current_exe)` while NSIS
/// was already replacing the locked binary).
#[tauri::command]
pub async fn install_pending_update(
    app: AppHandle,
    pending: State<'_, PendingUpdate>,
) -> Result<(), String> {
    let Some((update, bytes)) = pending.take() else {
        return Err("no pending update — run check first".to_string());
    };
    update
        .install(bytes)
        .map_err(|e| format!("update install: {}", e))?;

    // Windows: NSIS in `installMode: "passive"` auto-launches the new binary
    // after replacing files. We must exit cleanly so the .exe lock releases
    // — DO NOT spawn a new process here, NSIS will.
    //
    // macOS / Linux AppImage: the bundle/AppImage is replaced in place, but
    // no auto-restart happens, so we restart explicitly.
    #[cfg(target_os = "windows")]
    {
        let _ = app; // suppress unused on Windows
        std::process::exit(0);
    }
    #[cfg(not(target_os = "windows"))]
    {
        app.restart();
    }
}
