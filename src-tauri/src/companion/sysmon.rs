// GET /v1/sys/metrics — a point-in-time host snapshot for the mobile
// System Monitor: CPU, memory, per-volume storage, uptime, and battery.
// All shapes camelCase per the mobile spec.

use std::time::Duration;

use axum::response::Json;
use serde::Serialize;
use serde_json::Value;
use sysinfo::{Disks, System};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Cpu {
    usage_pct: f32,
    brand: String,
    cores: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Memory {
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Battery {
    percent: u8,
    charging: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Volume {
    name: String,
    mount_point: String,
    total_bytes: u64,
    used_bytes: u64,
    available_bytes: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Metrics {
    server_name: String,
    platform: &'static str,
    uptime_secs: u64,
    cpu: Cpu,
    memory: Memory,
    battery: Option<Battery>,
    volumes: Vec<Volume>,
}

pub async fn sys_metrics() -> Json<Value> {
    let metrics = collect().await;
    Json(serde_json::to_value(metrics).unwrap_or_else(|_| serde_json::json!({})))
}

async fn collect() -> Metrics {
    let mut sys = System::new();
    // CPU usage needs two samples spaced apart; the first call only primes it.
    sys.refresh_cpu_all();
    tokio::time::sleep(Duration::from_millis(200)).await;
    sys.refresh_cpu_all();
    sys.refresh_memory();

    let cpus = sys.cpus();
    let brand = cpus
        .first()
        .map(|c| c.brand().trim().to_string())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "CPU".to_string());
    let cpu = Cpu {
        usage_pct: sys.global_cpu_usage(),
        brand,
        cores: cpus.len(),
    };

    let memory = Memory {
        total_bytes: sys.total_memory(),
        used_bytes: sys.used_memory(),
        available_bytes: sys.available_memory(),
    };

    let disks = Disks::new_with_refreshed_list();
    let mut volumes: Vec<Volume> = disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| {
            let total = d.total_space();
            let avail = d.available_space();
            let raw_name = d.name().to_string_lossy().to_string();
            let mount = d.mount_point().to_string_lossy().to_string();
            Volume {
                name: if raw_name.trim().is_empty() { mount.clone() } else { raw_name },
                mount_point: mount,
                total_bytes: total,
                used_bytes: total.saturating_sub(avail),
                available_bytes: avail,
            }
        })
        .collect();
    // Drop duplicate mount points (macOS lists firmlinked volumes twice).
    // dedup_by only collapses *adjacent* dupes, so track seen mounts explicitly.
    let mut seen_mounts = std::collections::HashSet::new();
    volumes.retain(|v| seen_mounts.insert(v.mount_point.clone()));

    Metrics {
        server_name: tauri_plugin_os::hostname(),
        platform: std::env::consts::OS,
        uptime_secs: System::uptime(),
        cpu,
        memory,
        battery: read_battery(),
        volumes,
    }
}

/// First battery's charge + charging state, or `None` when no battery exists
/// (desktops) or the platform query fails — the UI simply hides the chip.
fn read_battery() -> Option<Battery> {
    let manager = battery::Manager::new().ok()?;
    let battery = manager.batteries().ok()?.next()?.ok()?;
    let percent = (battery.state_of_charge().value * 100.0).round().clamp(0.0, 100.0) as u8;
    let charging = matches!(
        battery.state(),
        battery::State::Charging | battery::State::Full,
    );
    Some(Battery { percent, charging })
}
