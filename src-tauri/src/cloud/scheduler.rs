// Debounced auto-sync: mutation commands call `bump(kind)` after their SQL
// succeeds; this module collapses bursts into a single push 5s after the last
// bump. Per-kind dirty flags so we only push what actually changed.
//
// Implementation note: the bump path uses module-level statics (no AppHandle
// needed) so call sites are one-liners. The spawn task captures the AppHandle
// once at boot.

use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Notify;

const DEBOUNCE: Duration = Duration::from_millis(5_000);

static DIRTY: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
// Kinds drained from DIRTY but whose push hasn't finished yet. Without this,
// quitting mid-push would persist an empty pending set and lose the kinds if
// the push never completes.
static IN_FLIGHT: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
static ENABLED: OnceLock<Mutex<bool>> = OnceLock::new();
static NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

fn dirty() -> &'static Mutex<HashSet<&'static str>> {
    DIRTY.get_or_init(|| Mutex::new(HashSet::new()))
}
fn in_flight() -> &'static Mutex<HashSet<&'static str>> {
    IN_FLIGHT.get_or_init(|| Mutex::new(HashSet::new()))
}
fn enabled_flag() -> &'static Mutex<bool> {
    ENABLED.get_or_init(|| Mutex::new(false))
}
fn notify() -> Arc<Notify> {
    NOTIFY.get_or_init(|| Arc::new(Notify::new())).clone()
}

/// Tauri-state-managed handle. Methods proxy to the module statics — having
/// the type managed makes the surface explicit and keeps the API symmetric
/// with `AuthState`.
#[derive(Default)]
pub struct Scheduler;

impl Scheduler {
    pub fn enable(&self) {
        *enabled_flag().lock() = true;
    }
    pub fn disable_and_clear(&self) {
        *enabled_flag().lock() = false;
        dirty().lock().clear();
        in_flight().lock().clear();
        notify().notify_one();
    }
}

/// Mark a kind dirty. Cheap. Safe to call from any thread. Drops silently if
/// the scheduler isn't enabled yet (e.g. before the user signs in).
pub fn bump(kind: &'static str) {
    if !*enabled_flag().lock() {
        return;
    }
    dirty().lock().insert(kind);
    notify().notify_one();
}

fn drain() -> Vec<&'static str> {
    let kinds: Vec<&'static str> = dirty().lock().drain().collect();
    // Track the drained kinds as in-flight so the quit path still sees them
    // as pending until the push settles one way or the other.
    in_flight().lock().extend(kinds.iter().copied());
    kinds
}

/// Non-destructive view of everything still owed to the cloud: kinds waiting
/// in the dirty set plus kinds mid-push. Used by the quit path to persist
/// pending kinds so they survive a restart.
pub fn pending_kinds() -> Vec<&'static str> {
    let mut set: HashSet<&'static str> = dirty().lock().iter().copied().collect();
    set.extend(in_flight().lock().iter().copied());
    set.into_iter().collect()
}

/// Re-mark kinds dirty by slug (boot-time recovery of a persisted set).
/// Unknown slugs are ignored. Uses the canonical &'static strs from
/// ALL_KINDS so the HashSet<&'static str> type holds.
pub fn rebump_slugs(slugs: &[String]) {
    for s in slugs {
        if let Some(k) = crate::cloud::domains::ALL_KINDS
            .iter()
            .copied()
            .find(|k| *k == s.as_str())
        {
            bump(k);
        }
    }
}

/// Spawn the scheduler loop. Lives for the app's lifetime.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Hold the Notify Arc for the loop's lifetime so .notified() futures
        // don't try to borrow from a temporary.
        let n = notify();
        loop {
            n.notified().await;

            // Debounce: keep resetting the timer while more bumps arrive.
            loop {
                let timer = tokio::time::sleep(DEBOUNCE);
                tokio::pin!(timer);
                tokio::select! {
                    _ = &mut timer => break,
                    _ = n.notified() => continue,
                }
            }

            let kinds = drain();
            if kinds.is_empty() {
                continue;
            }

            let pool = match app.try_state::<sqlx::SqlitePool>() {
                Some(p) => p.inner().clone(),
                None => {
                    log::warn!("[cloud:scheduler] SqlitePool state missing; dropping {} kinds", kinds.len());
                    continue;
                }
            };
            let auth_state = app.state::<crate::cloud::auth::AuthState>();
            if !auth_state.is_connected() {
                // Re-mark dirty for when the user comes back. Back in dirty
                // means no longer in-flight.
                for k in &kinds {
                    dirty().lock().insert(k);
                    in_flight().lock().remove(k);
                }
                continue;
            }

            let kind_slice: Vec<&str> = kinds.iter().copied().collect();

            // Snapshot conflicted kinds before — used to detect new conflicts
            // post-push without plumbing AppHandle through sync.rs.
            let pre_conflicts = crate::cloud::sync::conflicted_kinds(&pool)
                .await
                .unwrap_or_default();

            match crate::cloud::sync::push_all(&pool, &auth_state, &kind_slice).await {
                Ok(pushed) => {
                    // `pushed` only lists kinds that moved bytes, but every
                    // drained kind was handled (NoChange/Conflict included) —
                    // clear them all from in-flight.
                    for k in &kinds {
                        in_flight().lock().remove(k);
                    }
                    if !pushed.is_empty() {
                        let _ = app.emit("cloud:synced", &pushed);
                        log::info!("[cloud:scheduler] auto-pushed: {:?}", pushed);
                    }
                }
                Err(e) => {
                    log::warn!("[cloud:scheduler] push failed ({}); re-queueing", e);
                    // Back in dirty means no longer in-flight.
                    for k in &kinds {
                        dirty().lock().insert(k);
                        in_flight().lock().remove(k);
                    }
                }
            }

            // If the conflict set changed (new conflicts appeared OR an
            // existing one was cleared by a successful push), tell the
            // frontend so it can refresh the resolver + amber-dot state.
            let post_conflicts = crate::cloud::sync::conflicted_kinds(&pool)
                .await
                .unwrap_or_default();
            if post_conflicts != pre_conflicts {
                let _ = app.emit("cloud:conflicts-changed", &post_conflicts);
                log::info!("[cloud:scheduler] conflicts now: {:?}", post_conflicts);
            }
        }
    });
}
