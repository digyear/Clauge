pub mod models;
pub use models::*;

pub mod storage;
use storage::*;

pub mod profiles;
pub mod git;
pub mod worktree;

use base64::Engine;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::Read;
use std::path::PathBuf;
use tauri::ipc::Channel;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{TrayIconBuilder, TrayIconId};
use tauri::{Manager, State};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Get all profiles — no auto-discovery of session IDs
/// Session IDs are only set when explicitly captured after a session starts
#[tauri::command]
fn refresh_session_ids() -> Result<Vec<SessionProfile>, String> {
    Ok(load_profiles())
}

/// Update the claude session ID for a specific profile
#[tauri::command]
fn update_session_id(id: String, claude_session_id: String) -> Result<(), String> {
    let mut profiles = load_profiles();
    if let Some(profile) = profiles.iter_mut().find(|p| p.id == id) {
        profile.claude_session_id = Some(claude_session_id);
    } else {
        return Err("Profile not found".to_string());
    }
    save_profiles(&profiles)?;
    Ok(())
}

#[tauri::command]
async fn get_usage_analytics(days: Option<u32>) -> Result<UsageAnalytics, String> {
    tauri::async_runtime::spawn_blocking(move || get_usage_analytics_sync(days))
        .await
        .map_err(|e| format!("Thread error: {}", e))?
}

fn get_usage_analytics_sync(days: Option<u32>) -> Result<UsageAnalytics, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let projects_dir = home.join(".claude").join("projects");

    if !projects_dir.exists() {
        return Ok(UsageAnalytics {
            total_cost: 0.0, total_input_tokens: 0, total_output_tokens: 0,
            total_cache_read_tokens: 0, total_cache_write_tokens: 0,
            total_sessions: 0, total_api_calls: 0, cache_hit_percent: 0.0,
            daily: vec![], by_model: vec![], by_project: vec![],
            top_sessions: vec![], tools: vec![], shell_commands: vec![],
        });
    }

    let days_limit = days.unwrap_or(30);
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days_limit as i64);

    // Pricing per million tokens (approximate Claude pricing)
    let price_for_model = |model: &str| -> (f64, f64, f64, f64) {
        // (input, output, cache_read, cache_write) per million tokens
        let m = model.to_lowercase();
        if m.contains("opus") { (15.0, 75.0, 1.5, 18.75) }
        else if m.contains("haiku") { (0.80, 4.0, 0.08, 1.0) }
        else { (3.0, 15.0, 0.3, 3.75) } // sonnet default
    };

    let mut total_input: u64 = 0;
    let mut total_output: u64 = 0;
    let mut total_cache_read: u64 = 0;
    let mut total_cache_write: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut total_calls: u32 = 0;
    let mut total_sessions: u32 = 0;

    let mut daily_map: std::collections::HashMap<String, (f64, u32, u64, u64)> = std::collections::HashMap::new();
    let mut model_map: std::collections::HashMap<String, (f64, u32, u64, u64, u64, u64)> = std::collections::HashMap::new();
    let mut project_map: std::collections::HashMap<String, (f64, u32, u32)> = std::collections::HashMap::new();
    let mut session_costs: Vec<SessionCost> = Vec::new();
    let mut tool_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut shell_map: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    // Iterate all project directories
    for project_entry in std::fs::read_dir(&projects_dir).map_err(|e| e.to_string())?.flatten() {
        let project_name = project_entry.file_name().to_string_lossy().to_string();
        let project_dir = project_entry.path();
        if !project_dir.is_dir() { continue; }

        let mut project_cost: f64 = 0.0;
        let mut project_sessions: u32 = 0;
        let mut project_calls: u32 = 0;

        // Iterate session files
        for session_entry in std::fs::read_dir(&project_dir).map_err(|e| e.to_string())?.flatten() {
            let path = session_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") { continue; }

            // Check modification time
            if let Ok(metadata) = path.metadata() {
                if let Ok(modified) = metadata.modified() {
                    let modified_time: chrono::DateTime<chrono::Utc> = modified.into();
                    if modified_time < cutoff { continue; }
                }
            }

            let session_id = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            let mut session_cost: f64 = 0.0;
            let mut session_calls: u32 = 0;
            let mut session_model = String::new();
            total_sessions += 1;
            project_sessions += 1;

            for line in content.lines() {
                if line.trim().is_empty() { continue; }
                let val: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                // Extract model and usage from assistant messages
                let msg_type = val.get("type").and_then(|v| v.as_str()).unwrap_or("");
                if msg_type != "assistant" { continue; }

                let message = match val.get("message") {
                    Some(m) => m,
                    None => continue,
                };

                let model = message.get("model").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                if session_model.is_empty() { session_model = model.clone(); }

                let usage = match message.get("usage") {
                    Some(u) => u,
                    None => continue,
                };

                let input = usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_write = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);

                let (pi, po, pcr, pcw) = price_for_model(&model);
                let call_cost = (input as f64 * pi + output as f64 * po + cache_read as f64 * pcr + cache_write as f64 * pcw) / 1_000_000.0;

                total_input += input;
                total_output += output;
                total_cache_read += cache_read;
                total_cache_write += cache_write;
                total_cost += call_cost;
                total_calls += 1;
                session_cost += call_cost;
                session_calls += 1;
                project_cost += call_cost;
                project_calls += 1;

                // Daily
                // Try to get timestamp from the entry
                let date_str = val.get("timestamp").and_then(|v| v.as_str())
                    .map(|t| t[..10].to_string())
                    .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());
                let daily = daily_map.entry(date_str).or_insert((0.0, 0, 0, 0));
                daily.0 += call_cost;
                daily.1 += 1;
                daily.2 += input;
                daily.3 += output;

                // Model
                let short_model = if model.contains("opus") { "Opus".to_string() }
                    else if model.contains("haiku") { "Haiku".to_string() }
                    else if model.contains("sonnet") { "Sonnet".to_string() }
                    else { model.clone() };
                let me = model_map.entry(short_model).or_insert((0.0, 0, 0, 0, 0, 0));
                me.0 += call_cost;
                me.1 += 1;
                me.2 += input;
                me.3 += output;
                me.4 += cache_read;
                me.5 += cache_write;

                // Tools
                if let Some(content_arr) = message.get("content").and_then(|v| v.as_array()) {
                    for block in content_arr {
                        if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                            let tool_name = block.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();
                            *tool_map.entry(tool_name.clone()).or_insert(0) += 1;

                            // Extract shell commands from Bash tool
                            if tool_name == "Bash" || tool_name == "bash" {
                                if let Some(input_obj) = block.get("input") {
                                    if let Some(cmd) = input_obj.get("command").and_then(|v| v.as_str()) {
                                        let shell_cmd = cmd.split_whitespace().next().unwrap_or("").to_string();
                                        if !shell_cmd.is_empty() {
                                            *shell_map.entry(shell_cmd).or_insert(0) += 1;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if session_calls > 0 {
                session_costs.push(SessionCost {
                    session_id,
                    project: project_name.clone(),
                    cost: session_cost,
                    calls: session_calls,
                    model: session_model,
                });
            }
        }

        if project_sessions > 0 {
            project_map.insert(project_name, (project_cost, project_sessions, project_calls));
        }
    }

    // Sort and format results
    let mut daily: Vec<DailyUsage> = daily_map.into_iter().map(|(date, (cost, calls, input, output))| {
        DailyUsage { date, cost, calls, input_tokens: input, output_tokens: output }
    }).collect();
    daily.sort_by(|a, b| a.date.cmp(&b.date));

    let mut by_model: Vec<ModelUsage> = model_map.into_iter().map(|(model, (cost, calls, input, output, cr, cw))| {
        let total_input_for_model = input + cr + cw;
        let cache_pct = if total_input_for_model > 0 { (cr as f64 / total_input_for_model as f64) * 100.0 } else { 0.0 };
        ModelUsage { model, cost, calls, input_tokens: input, output_tokens: output, cache_hit_percent: cache_pct }
    }).collect();
    by_model.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));

    let mut by_project: Vec<ProjectUsage> = project_map.into_iter().map(|(project, (cost, sessions, calls))| {
        ProjectUsage { project, cost, sessions, calls }
    }).collect();
    by_project.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));

    session_costs.sort_by(|a, b| b.cost.partial_cmp(&a.cost).unwrap_or(std::cmp::Ordering::Equal));
    let top_sessions = session_costs.into_iter().take(5).collect();

    let mut tools: Vec<ToolCount> = tool_map.into_iter().map(|(name, count)| ToolCount { name, count }).collect();
    tools.sort_by(|a, b| b.count.cmp(&a.count));

    let mut shell_commands: Vec<ToolCount> = shell_map.into_iter().map(|(name, count)| ToolCount { name, count }).collect();
    shell_commands.sort_by(|a, b| b.count.cmp(&a.count));
    shell_commands.truncate(15);

    let total_all_input = total_input + total_cache_read + total_cache_write;
    let cache_hit_percent = if total_all_input > 0 { (total_cache_read as f64 / total_all_input as f64) * 100.0 } else { 0.0 };

    Ok(UsageAnalytics {
        total_cost,
        total_input_tokens: total_input,
        total_output_tokens: total_output,
        total_cache_read_tokens: total_cache_read,
        total_cache_write_tokens: total_cache_write,
        total_sessions,
        total_api_calls: total_calls,
        cache_hit_percent,
        daily,
        by_model,
        by_project,
        top_sessions,
        tools,
        shell_commands,
    })
}

/// Count active sessions for a project path (profiles that have been used)
#[tauri::command]
fn count_project_sessions(project_path: String) -> Result<u32, String> {
    let profiles = load_profiles();
    let count = profiles.iter()
        .filter(|p| p.project_path == project_path)
        .count() as u32;
    Ok(count)
}

#[tauri::command]
fn discover_sessions(project_path: String) -> Result<Vec<DiscoveredSession>, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let encoded = encode_project_path(&project_path);
    let projects_dir = home.join(".claude").join("projects").join(&encoded);

    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let entries = std::fs::read_dir(&projects_dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let session_id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            let modified_at = path
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .map(|t| {
                    let datetime: chrono::DateTime<chrono::Utc> = t.into();
                    datetime.to_rfc3339()
                })
                .unwrap_or_default();

            // Extract first user message as preview
            let preview = std::fs::read_to_string(&path).ok().and_then(|content| {
                for line in content.lines().take(20) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
                        if val.get("type").and_then(|t| t.as_str()) == Some("human") {
                            if let Some(msg) = val.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                                let trimmed = msg.chars().take(80).collect::<String>();
                                return Some(trimmed);
                            }
                        }
                    }
                }
                None
            });

            sessions.push(DiscoveredSession {
                session_id,
                modified_at,
                preview,
            });
        }
    }

    sessions.sort_by(|a, b| b.modified_at.cmp(&a.modified_at));
    Ok(sessions)
}

#[tauri::command]
fn get_session_tokens(
    project_path: String,
    session_id: Option<String>,
) -> Result<TokenUsage, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let encoded = encode_project_path(&project_path);
    let projects_dir = home.join(".claude").join("projects").join(&encoded);

    if !projects_dir.exists() {
        return Err("Project directory not found".to_string());
    }

    let file_path = if let Some(sid) = session_id {
        projects_dir.join(format!("{}.jsonl", sid))
    } else {
        // Find most recent .jsonl file
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        if let Ok(entries) = std::fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Ok(meta) = path.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if best.as_ref().map_or(true, |(_, t)| modified > *t) {
                                best = Some((path, modified));
                            }
                        }
                    }
                }
            }
        }
        best.map(|(p, _)| p)
            .ok_or("No session files found")?
    };

    if !file_path.exists() {
        return Err("Session file not found".to_string());
    }

    let contents = std::fs::read_to_string(&file_path).map_err(|e| e.to_string())?;

    let mut input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut cache_read_tokens: u64 = 0;
    let mut cache_creation_tokens: u64 = 0;

    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            // Check both direct usage and message.usage patterns
            let usage = val.get("usage").or_else(|| {
                val.get("message").and_then(|m| m.get("usage"))
            });
            if let Some(u) = usage {
                input_tokens += u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                output_tokens += u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                cache_read_tokens += u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                cache_creation_tokens += u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            }
        }
    }

    let total_tokens = input_tokens + output_tokens + cache_read_tokens + cache_creation_tokens;

    Ok(TokenUsage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_tokens,
    })
}


/// Fetch usage limits via reqwest with native-tls (uses macOS SecureTransport to bypass Cloudflare)
#[tauri::command]
fn fetch_usage_limits(session_key: String) -> Result<serde_json::Value, String> {
    let client = reqwest::blocking::Client::builder()
        .use_native_tls()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
    let cookie = format!("sessionKey={}", session_key);

    // Step 1: Get org ID
    let orgs_resp = client
        .get("https://claude.ai/api/organizations")
        .header("Cookie", &cookie)
        .header("User-Agent", ua)
        .send()
        .map_err(|e| format!("orgs request failed: {}", e))?;

    let orgs: Vec<serde_json::Value> = orgs_resp
        .json()
        .map_err(|e| format!("orgs parse failed: {}", e))?;

    let org_id = orgs
        .first()
        .and_then(|o| o.get("uuid"))
        .and_then(|v| v.as_str())
        .ok_or("No organization found")?
        .to_string();

    // Step 2: Get usage
    let usage_resp = client
        .get(&format!("https://claude.ai/api/organizations/{}/usage", org_id))
        .header("Cookie", &cookie)
        .header("User-Agent", ua)
        .send()
        .map_err(|e| format!("usage request failed: {}", e))?;

    let usage: serde_json::Value = usage_resp
        .json()
        .map_err(|e| format!("usage parse failed: {}", e))?;

    Ok(usage)
}

/// Save session key to local storage
#[tauri::command]
fn save_session_key(key: String) -> Result<(), String> {
    let path = get_storage_dir().join("session_key");
    std::fs::write(&path, &key).map_err(|e| e.to_string())
}

/// Load session key from local storage
#[tauri::command]
fn load_session_key() -> Result<Option<String>, String> {
    let path = get_storage_dir().join("session_key");
    if path.exists() {
        let key = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let key = key.trim().to_string();
        if key.is_empty() { Ok(None) } else { Ok(Some(key)) }
    } else {
        Ok(None)
    }
}

/// Get app version from Cargo.toml
#[tauri::command]
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Get Claude subscription plan from keychain
#[tauri::command]
fn get_claude_plan() -> Result<String, String> {
    let output = std::process::Command::new("security")
        .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
        .output()
        .map_err(|e| format!("Keychain error: {}", e))?;
    if !output.status.success() { return Ok(String::new()); }
    let json_str = String::from_utf8(output.stdout).map_err(|e| e.to_string())?;
    let parsed: serde_json::Value = serde_json::from_str(json_str.trim()).map_err(|e| e.to_string())?;
    Ok(parsed.get("claudeAiOauth")
        .and_then(|o| o.get("subscriptionType").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string())
}

/// Update the tray title text (shown in menu bar)
#[tauri::command]
fn update_tray_title(app_handle: tauri::AppHandle, title: String) -> Result<(), String> {
    if let Some(tray) = app_handle.tray_by_id(&TrayIconId::new("main-tray")) {
        tray.set_title(Some(&title)).map_err(|e| format!("Tray title error: {}", e))?;
    }
    Ok(())
}


/// Resolve the full path to the `claude` binary by asking the user's login shell.
/// Falls back to just "claude" if resolution fails.
fn resolve_claude_path() -> String {
    let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    if let Ok(output) = std::process::Command::new(&user_shell)
        .args(["-l", "-i", "-c", "which claude"])
        .output()
    {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return path;
            }
        }
    }
    "claude".to_string()
}

/// Spawn a terminal using Tauri's Channel API for streaming PTY output to the frontend.
/// The `on_output` channel sends TerminalOutputPayload messages directly to JS callback.
#[tauri::command]
fn spawn_terminal(
    state: State<'_, TerminalState>,
    session_id: Option<String>,
    project_path: String,
    context_prompt: Option<String>,
    skip_permissions: Option<bool>,
    git_name: Option<String>,
    git_email: Option<String>,
    on_output: Channel<TerminalOutputPayload>,
) -> Result<String, String> {
    let terminal_id = Uuid::new_v4().to_string();

    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    // Build the claude command string
    let mut claude_cmd = String::from("claude");
    if let Some(ref sid) = session_id {
        claude_cmd.push_str(&format!(" --resume \"{}\"", sid));
    }
    if skip_permissions.unwrap_or(false) {
        claude_cmd.push_str(" --dangerously-skip-permissions");
    }
    if let Some(ref prompt) = context_prompt {
        if !prompt.is_empty() {
            let escaped = prompt.replace('\\', "\\\\").replace('"', "\\\"");
            claude_cmd.push_str(&format!(" --append-system-prompt \"{}\"", escaped));
        }
    }

    eprintln!("[Clauge] Spawning command: {}", &claude_cmd[..claude_cmd.len().min(120)]);
    eprintln!("[Clauge] CWD: {}", project_path);

    // Spawn via login shell so full environment (nvm, node, etc.) is available
    let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = CommandBuilder::new(&user_shell);
    cmd.arg("-l");
    cmd.arg("-i");
    cmd.arg("-c");
    cmd.arg(&claude_cmd);
    cmd.cwd(&project_path);

    if let Some(home) = dirs::home_dir() {
        cmd.env("HOME", home.to_string_lossy().to_string());
    }
    cmd.env("TERM", "xterm-256color");

    if let Some(ref name) = git_name {
        cmd.env("GIT_AUTHOR_NAME", name);
        cmd.env("GIT_COMMITTER_NAME", name);
    }
    if let Some(ref email) = git_email {
        cmd.env("GIT_AUTHOR_EMAIL", email);
        cmd.env("GIT_COMMITTER_EMAIL", email);
    }

    let child = pty_pair
        .slave
        .spawn_command(cmd)
        .map_err(|e| {
            eprintln!("[Clauge] Spawn FAILED: {}", e);
            format!("Failed to spawn claude: {}", e)
        })?;

    eprintln!("[Clauge] Spawn SUCCESS, terminal_id: {}", terminal_id);

    let writer = pty_pair
        .master
        .take_writer()
        .map_err(|e| format!("Failed to get PTY writer: {}", e))?;

    let reader = pty_pair
        .master
        .try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

    let tid_clone = terminal_id.clone();
    std::thread::spawn(move || {
        eprintln!("[Clauge] Reader thread started for {}", tid_clone);
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        let mut total_bytes = 0usize;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    eprintln!("[Clauge] Reader EOF (total {} bytes)", total_bytes);
                    break;
                }
                Ok(n) => {
                    total_bytes += n;
                    if total_bytes == n {
                        eprintln!("[Clauge] First read: {} bytes", n);
                    }
                    let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    let payload = TerminalOutputPayload {
                        terminal_id: tid_clone.clone(),
                        data,
                    };
                    if let Err(e) = on_output.send(payload) {
                        eprintln!("[Clauge] Channel send error: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("[Clauge] Reader error: {}", e);
                    break;
                }
            }
        }
    });

    let entry = TerminalEntry {
        master: pty_pair.master,
        writer,
        child,
    };

    state
        .terminals
        .lock()
        .insert(terminal_id.clone(), entry);

    Ok(terminal_id)
}

/// Spawn a plain shell terminal (no Claude) in the given project directory
#[tauri::command]
fn spawn_shell(
    state: State<'_, TerminalState>,
    project_path: String,
    on_output: Channel<TerminalOutputPayload>,
) -> Result<String, String> {
    let terminal_id = Uuid::new_v4().to_string();
    let pty_system = native_pty_system();
    let pty_pair = pty_system
        .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
        .map_err(|e| format!("Failed to open PTY: {}", e))?;

    let user_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    let mut cmd = CommandBuilder::new(&user_shell);
    cmd.arg("-l");
    cmd.arg("-i");
    cmd.cwd(&project_path);
    if let Some(home) = dirs::home_dir() {
        cmd.env("HOME", home.to_string_lossy().to_string());
    }
    cmd.env("TERM", "xterm-256color");

    let child = pty_pair.slave.spawn_command(cmd)
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    let writer = pty_pair.master.take_writer()
        .map_err(|e| format!("Failed to get PTY writer: {}", e))?;
    let reader = pty_pair.master.try_clone_reader()
        .map_err(|e| format!("Failed to clone PTY reader: {}", e))?;

    let tid_clone = terminal_id.clone();
    std::thread::spawn(move || {
        let mut reader = reader;
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    let data = base64::engine::general_purpose::STANDARD.encode(&buf[..n]);
                    if on_output.send(TerminalOutputPayload { terminal_id: tid_clone.clone(), data }).is_err() { break; }
                }
                Err(_) => break,
            }
        }
    });

    state.terminals.lock()
        .insert(terminal_id.clone(), TerminalEntry { master: pty_pair.master, writer, child });

    Ok(terminal_id)
}

#[tauri::command]
fn write_to_terminal(
    state: State<'_, TerminalState>,
    terminal_id: String,
    data: String,
) -> Result<(), String> {
    let mut terminals = state
        .terminals
        .lock();

    let entry = terminals
        .get_mut(&terminal_id)
        .ok_or("Terminal not found")?;

    entry
        .writer
        .write_all(data.as_bytes())
        .map_err(|e| format!("Write error: {}", e))?;

    entry
        .writer
        .flush()
        .map_err(|e| format!("Flush error: {}", e))?;

    Ok(())
}

#[tauri::command]
fn resize_terminal(
    state: State<'_, TerminalState>,
    terminal_id: String,
    cols: u32,
    rows: u32,
) -> Result<(), String> {
    let terminals = state
        .terminals
        .lock();

    let entry = terminals.get(&terminal_id).ok_or("Terminal not found")?;

    entry
        .master
        .resize(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| format!("Resize error: {}", e))?;

    Ok(())
}

#[tauri::command]
fn kill_terminal(
    state: State<'_, TerminalState>,
    terminal_id: String,
) -> Result<(), String> {
    let mut terminals = state.terminals.lock();
    if let Some(mut entry) = terminals.remove(&terminal_id) {
        let _ = entry.child.kill();
        eprintln!("[Clauge] Killed terminal {}", terminal_id);
    }
    Ok(())
}

/// Get Claude Code plugins from settings.json + installed_plugins.json
#[tauri::command]
fn get_claude_plugins() -> Result<Vec<ClaudePlugin>, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let settings_path = home.join(".claude").join("settings.json");
    let installed_path = home.join(".claude").join("plugins").join("installed_plugins.json");

    // Read enabled plugins from settings
    let mut enabled_map: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        let settings: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if let Some(plugins) = settings.get("enabledPlugins").and_then(|v| v.as_object()) {
            for (key, val) in plugins {
                enabled_map.insert(key.clone(), val.as_bool().unwrap_or(false));
            }
        }
    }

    // Read installed plugin details
    let mut plugins = Vec::new();
    if installed_path.exists() {
        let content = std::fs::read_to_string(&installed_path).map_err(|e| e.to_string())?;
        let installed: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if let Some(plugin_map) = installed.get("plugins").and_then(|v| v.as_object()) {
            for (key, entries) in plugin_map {
                let parts: Vec<&str> = key.splitn(2, '@').collect();
                let name = parts.first().unwrap_or(&"").to_string();
                let marketplace = parts.get(1).unwrap_or(&"").to_string();

                let (version, install_path) = entries.as_array()
                    .and_then(|arr| arr.first())
                    .map(|entry| {
                        let v = entry.get("version").and_then(|v| v.as_str()).map(String::from);
                        let p = entry.get("installPath").and_then(|v| v.as_str()).map(String::from);
                        (v, p)
                    })
                    .unwrap_or((None, None));

                let enabled = enabled_map.get(key).copied().unwrap_or(false);

                plugins.push(ClaudePlugin {
                    name,
                    marketplace,
                    enabled,
                    version,
                    install_path,
                });
            }
        }
    }

    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

/// Toggle a Claude Code plugin on/off in settings.json
#[tauri::command]
fn toggle_claude_plugin(plugin_key: String, enabled: bool) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let settings_path = home.join(".claude").join("settings.json");

    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        serde_json::from_str(&content).map_err(|e| e.to_string())?
    } else {
        serde_json::json!({})
    };

    if settings.get("enabledPlugins").is_none() {
        settings["enabledPlugins"] = serde_json::json!({});
    }
    settings["enabledPlugins"][&plugin_key] = serde_json::Value::Bool(enabled);

    let json = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_path, json).map_err(|e| e.to_string())?;
    Ok(())
}

/// Browse available plugins from local marketplace caches
#[tauri::command]
fn get_marketplace_plugins() -> Result<Vec<MarketplacePlugin>, String> {
    let home = dirs::home_dir().ok_or("Cannot determine home directory")?;
    let marketplaces_dir = home.join(".claude").join("plugins").join("marketplaces");
    let installed_path = home.join(".claude").join("plugins").join("installed_plugins.json");

    // Get installed plugin keys
    let mut installed_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    if installed_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&installed_path) {
            if let Ok(installed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(plugins) = installed.get("plugins").and_then(|v| v.as_object()) {
                    for key in plugins.keys() {
                        installed_keys.insert(key.clone());
                    }
                }
            }
        }
    }

    // Read install counts cache
    let mut install_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let counts_path = home.join(".claude").join("plugins").join("install-counts-cache.json");
    if counts_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&counts_path) {
            if let Ok(cache) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(counts) = cache.get("counts").and_then(|v| v.as_array()) {
                    for entry in counts {
                        if let (Some(plugin), Some(count)) = (
                            entry.get("plugin").and_then(|v| v.as_str()),
                            entry.get("unique_installs").and_then(|v| v.as_u64()),
                        ) {
                            install_counts.insert(plugin.to_string(), count);
                        }
                    }
                }
            }
        }
    }

    let mut results = Vec::new();

    if !marketplaces_dir.exists() {
        return Ok(results);
    }

    // Scan each marketplace directory
    for entry in std::fs::read_dir(&marketplaces_dir).map_err(|e| e.to_string())?.flatten() {
        let marketplace_name = entry.file_name().to_string_lossy().to_string();
        let registry_path = entry.path().join(".claude-plugin").join("marketplace.json");

        if !registry_path.exists() {
            continue;
        }

        let content = match std::fs::read_to_string(&registry_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let registry: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        if let Some(plugins) = registry.get("plugins").and_then(|v| v.as_array()) {
            for plugin in plugins {
                let name = plugin.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() { continue; }
                let description = plugin.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let category = plugin.get("category").and_then(|v| v.as_str()).map(String::from);
                let key = format!("{}@{}", name, marketplace_name);
                let installed = installed_keys.contains(&key);

                let installs = install_counts.get(&key).copied();
                results.push(MarketplacePlugin {
                    name,
                    description,
                    marketplace: marketplace_name.clone(),
                    category,
                    installed,
                    installs,
                });
            }
        }
    }

    results.sort_by(|a, b| b.installs.unwrap_or(0).cmp(&a.installs.unwrap_or(0)));
    Ok(results)
}

/// Install a plugin via `claude plugins add`
#[tauri::command]
fn install_plugin(name: String, marketplace: String) -> Result<(), String> {
    let plugin_id = format!("{}@{}", name, marketplace);
    let output = std::process::Command::new("claude")
        .args(["plugins", "install", &plugin_id])
        .output()
        .map_err(|e| format!("Failed to run claude plugins install: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Install failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Uninstall a plugin via `claude plugins remove`
#[tauri::command]
fn uninstall_plugin(name: String, marketplace: String) -> Result<(), String> {
    let plugin_id = format!("{}@{}", name, marketplace);
    let output = std::process::Command::new("claude")
        .args(["plugins", "uninstall", &plugin_id])
        .output()
        .map_err(|e| format!("Failed to run claude plugins uninstall: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Uninstall failed: {}", stderr.trim()));
    }
    Ok(())
}

/// Get storage dir for context snippets
fn get_contexts_dir() -> PathBuf {
    let dir = get_storage_dir().join("contexts");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// List all saved context snippets
#[tauri::command]
fn get_context_snippets() -> Result<Vec<serde_json::Value>, String> {
    let dir = get_contexts_dir();
    let mut snippets = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("md") {
                let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
                let content = std::fs::read_to_string(&path).unwrap_or_default();
                let preview = content.lines().next().unwrap_or("").chars().take(80).collect::<String>();
                snippets.push(serde_json::json!({
                    "name": name,
                    "content": content,
                    "preview": preview,
                }));
            }
        }
    }
    snippets.sort_by(|a, b| a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or("")));
    Ok(snippets)
}

/// Save a context snippet
#[tauri::command]
fn save_context_snippet(name: String, content: String) -> Result<(), String> {
    let path = get_contexts_dir().join(format!("{}.md", name));
    std::fs::write(&path, &content).map_err(|e| e.to_string())
}

/// Delete a context snippet
#[tauri::command]
fn delete_context_snippet(name: String) -> Result<(), String> {
    let path = get_contexts_dir().join(format!("{}.md", name));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Inject context snippets into CLAUDE.md for a session
#[tauri::command]
fn inject_session_context(project_path: String, context_names: Vec<String>) -> Result<(), String> {
    let contexts_dir = get_contexts_dir();
    let mut combined = String::new();

    for name in &context_names {
        let path = contexts_dir.join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !combined.is_empty() { combined.push_str("\n\n---\n\n"); }
            combined.push_str(&format!("## {}\n\n{}", name, content));
        }
    }

    let claude_md_path = PathBuf::from(&project_path).join("CLAUDE.md");
    let marker_start = "<!-- CLAUGE-CONTEXT-START -->";
    let marker_end = "<!-- CLAUGE-CONTEXT-END -->";

    // Read existing content (without old markers) to check for duplicates
    let existing_content = if claude_md_path.exists() {
        let raw = std::fs::read_to_string(&claude_md_path).map_err(|e| e.to_string())?;
        if let (Some(start), Some(end)) = (raw.find(marker_start), raw.find(marker_end)) {
            raw[..start].trim_end().to_string()
        } else {
            raw
        }
    } else {
        String::new()
    };

    // Filter out snippets whose content already exists in the file
    let mut filtered = String::new();
    for name in &context_names {
        let path = contexts_dir.join(format!("{}.md", name));
        if let Ok(content) = std::fs::read_to_string(&path) {
            if !existing_content.contains(content.trim()) {
                if !filtered.is_empty() { filtered.push_str("\n\n---\n\n"); }
                filtered.push_str(&format!("## {}\n\n{}", name, content));
            }
        }
    }

    if filtered.is_empty() { return Ok(()); }

    let injected = format!("\n\n{}\n{}\n{}\n", marker_start, filtered, marker_end);

    if !existing_content.is_empty() {
        std::fs::write(&claude_md_path, format!("{}{}", existing_content.trim_end(), injected)).map_err(|e| e.to_string())?;
    } else {
        std::fs::write(&claude_md_path, filtered).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Remove injected context from CLAUDE.md
#[tauri::command]
fn remove_injected_context(project_path: String) -> Result<(), String> {
    let claude_md_path = PathBuf::from(&project_path).join("CLAUDE.md");
    if !claude_md_path.exists() { return Ok(()); }

    let content = std::fs::read_to_string(&claude_md_path).map_err(|e| e.to_string())?;
    let marker_start = "<!-- CLAUGE-CONTEXT-START -->";
    let marker_end = "<!-- CLAUGE-CONTEXT-END -->";

    if let (Some(start), Some(end)) = (content.find(marker_start), content.find(marker_end)) {
        let cleaned = format!("{}{}", &content[..start].trim_end(), &content[end + marker_end.len()..]);
        if cleaned.trim().is_empty() {
            // We created this file — delete it
            let _ = std::fs::remove_file(&claude_md_path);
        } else {
            std::fs::write(&claude_md_path, cleaned.trim_end().to_string() + "\n").map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

/// Update contexts attached to a session profile
#[tauri::command]
fn update_session_contexts(id: String, contexts: Vec<String>) -> Result<(), String> {
    let mut profiles = load_profiles();
    if let Some(profile) = profiles.iter_mut().find(|p| p.id == id) {
        profile.contexts = contexts;
    } else {
        return Err("Profile not found".to_string());
    }
    save_profiles(&profiles)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .manage(TerminalState::default())
        .invoke_handler(tauri::generate_handler![
            profiles::get_profiles,
            profiles::create_profile,
            profiles::delete_profile,
            profiles::rename_profile,
            profiles::update_last_used,
            refresh_session_ids,
            update_session_id,
            worktree::is_git_repo,
            git::get_git_status,
            git::get_git_branch,
            git::get_git_ahead_behind,
            git::git_commit,
            git::git_push,
            git::git_pull,
            git::git_diff_file,
            git::git_stage_file,
            git::git_unstage_file,
            git::git_log,
            git::git_stash,
            git::git_stash_pop,
            git::git_list_branches,
            git::git_switch_branch,
            worktree::create_worktree,
            worktree::remove_worktree,
            worktree::update_profile_worktree,
            count_project_sessions,
            discover_sessions,
            get_session_tokens,
            fetch_usage_limits,
            get_usage_analytics,
            get_app_version,
            get_claude_plan,
            update_tray_title,
            save_session_key,
            load_session_key,
            spawn_terminal,
            spawn_shell,
            write_to_terminal,
            resize_terminal,
            kill_terminal,
            get_claude_plugins,
            toggle_claude_plugin,
            get_marketplace_plugins,
            install_plugin,
            uninstall_plugin,
            get_context_snippets,
            save_context_snippet,
            delete_context_snippet,
            inject_session_context,
            remove_injected_context,
            update_session_contexts
        ])
        .setup(|app| {
            let setup_start = std::time::Instant::now();
            eprintln!("[TIMING] setup start");

            let window = app.get_webview_window("main").unwrap();

            #[cfg(target_os = "macos")]
            {
                use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
                apply_vibrancy(&window, NSVisualEffectMaterial::Sidebar, None, None)
                    .expect("Failed to apply vibrancy");
            }
            eprintln!("[TIMING] vibrancy applied: {:?}", setup_start.elapsed());

            // ---- App menu bar ----
            let app_menu = Submenu::with_items(app, "Clauge", true, &[
                &PredefinedMenuItem::about(app, Some("About Clauge"), None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::services(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::quit(app, None)?,
            ])?;
            let edit_menu = Submenu::with_items(app, "Edit", true, &[
                &PredefinedMenuItem::undo(app, None)?,
                &PredefinedMenuItem::redo(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::cut(app, None)?,
                &PredefinedMenuItem::copy(app, None)?,
                &PredefinedMenuItem::paste(app, None)?,
                &PredefinedMenuItem::select_all(app, None)?,
            ])?;
            let window_menu = Submenu::with_items(app, "Window", true, &[
                &PredefinedMenuItem::minimize(app, None)?,
                &PredefinedMenuItem::maximize(app, None)?,
                &PredefinedMenuItem::separator(app)?,
                &PredefinedMenuItem::close_window(app, None)?,
            ])?;
            let menu_bar = Menu::with_items(app, &[&app_menu, &edit_menu, &window_menu])?;
            app.set_menu(menu_bar)?;

            // ---- System tray ----
            let show_item = MenuItem::with_id(app, "show", "Back to App", true, None::<&str>)?;
            let separator = PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&show_item, &separator, &quit])?;

            // Use custom tray icon — template mode so macOS adapts to light/dark menu bar
            let icon_png = include_bytes!("../icons/tray-dark.png");
            let img = image::load_from_memory(icon_png).expect("Failed to load tray icon");
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            let tray_icon = tauri::image::Image::new_owned(rgba.into_raw(), w, h);
            TrayIconBuilder::with_id("main-tray")
                .icon(tray_icon)
                .icon_as_template(true)
                .menu(&menu)
                .title("Clauge")
                .tooltip("Clauge — Claude Session Manager")
                .on_menu_event(move |app: &tauri::AppHandle, event: tauri::menu::MenuEvent| {
                    let id = event.id().as_ref();
                    if id == "quit" {
                        app.exit(0);
                    } else if id == "show" {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            eprintln!("[TIMING] tray built: {:?}", setup_start.elapsed());

            // Enable autostart on first run
            use tauri_plugin_autostart::ManagerExt;
            let _ = app.autolaunch().enable();

            eprintln!("[TIMING] setup complete: {:?}", setup_start.elapsed());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Hide instead of quit — user can quit from tray
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            match event {
                tauri::RunEvent::Reopen { .. } => {
                    if let Some(window) = app.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                tauri::RunEvent::ExitRequested { .. } => {
                    if let Some(state) = app.try_state::<TerminalState>() {
                        let mut terminals = state.terminals.lock();
                        for (id, mut entry) in terminals.drain() {
                            let _ = entry.child.kill();
                            eprintln!("[Clauge] Cleaned up terminal {} on exit", id);
                        }
                    }
                }
                _ => {}
            }
        });
}
