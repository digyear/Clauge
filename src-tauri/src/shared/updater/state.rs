//! In-process state for a pending update.
//!
//! The frontend's flow is two-step: `check_for_update_in_channel()` finds
//! and pre-downloads the next version, returning version + release-notes
//! metadata; `install_pending_update()` runs the install. Between the two
//! calls we have to keep the `Update` object **and the downloaded bytes**
//! alive in Rust state so we can reuse them (the JS side never sees the
//! typed Update — only metadata).
//!
//! `tauri_plugin_updater::Update::install(bytes)` requires the same bytes
//! that `Update::download()` returned — the Rust `Update` does not stash
//! them internally (unlike the JS plugin's Update object). Passing an
//! empty Vec causes `extract()` to fail on every platform.

use parking_lot::Mutex;
use std::sync::Arc;
use tauri_plugin_updater::Update;

#[derive(Default)]
pub struct PendingUpdate {
    inner: Arc<Mutex<Option<(Update, Vec<u8>)>>>,
}

impl PendingUpdate {
    pub fn store(&self, update: Update, bytes: Vec<u8>) {
        *self.inner.lock() = Some((update, bytes));
    }

    pub fn take(&self) -> Option<(Update, Vec<u8>)> {
        self.inner.lock().take()
    }
}
