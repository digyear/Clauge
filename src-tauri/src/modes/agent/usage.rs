use crate::modes::agent::models::*;
use crate::shared::cli::{claude::CLAUDE, runner::CliRunner};
use std::fs;
use std::path::PathBuf;

#[tauri::command]
pub async fn agent_get_usage_analytics(days: Option<u32>) -> Result<UsageAnalytics, String> {
    tauri::async_runtime::spawn_blocking(move || agent_get_usage_analytics_sync(days))
        .await
        .map_err(|e| format!("Thread error: {}", e))?
}

pub fn agent_get_usage_analytics_sync(days: Option<u32>) -> Result<UsageAnalytics, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let projects_dir = cli.sessions_root().ok_or("Cannot determine home directory")?;

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
            if !cli.is_session_file(&path) { continue; }

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

/// Fetch usage limits via reqwest with native-tls (uses macOS SecureTransport to bypass Cloudflare)
#[tauri::command]
pub async fn agent_fetch_usage_limits(session_key: String) -> Result<serde_json::Value, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let orgs_url = cli.usage_api_orgs_url().ok_or("CLI does not expose a usage API")?;

    let client = reqwest::Client::builder()
        .use_native_tls()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let ua = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15";
    let cookie = format!("sessionKey={}", session_key);

    // Step 1: Get org ID
    let orgs_resp = client
        .get(&orgs_url)
        .header("Cookie", &cookie)
        .header("User-Agent", ua)
        .send()
        .await
        .map_err(|e| format!("orgs request failed: {}", e))?;

    if !orgs_resp.status().is_success() {
        return Err(usage_auth_error("organization", orgs_resp.status()));
    }

    let orgs: Vec<serde_json::Value> = orgs_resp
        .json()
        .await
        .map_err(|e| format!("orgs parse failed: {}", e))?;

    let org_id = orgs
        .first()
        .and_then(|o: &serde_json::Value| o.get("uuid"))
        .and_then(|v: &serde_json::Value| v.as_str())
        .ok_or("No organization found")?
        .to_string();

    // Step 2: Get usage
    let usage_url = cli
        .usage_api_url_for(&org_id)
        .ok_or("CLI does not expose a per-org usage URL")?;
    let usage_resp = client
        .get(&usage_url)
        .header("Cookie", &cookie)
        .header("User-Agent", ua)
        .send()
        .await
        .map_err(|e| format!("usage request failed: {}", e))?;

    if !usage_resp.status().is_success() {
        return Err(usage_auth_error("usage", usage_resp.status()));
    }

    let usage: serde_json::Value = usage_resp
        .json()
        .await
        .map_err(|e| format!("usage parse failed: {}", e))?;

    Ok(usage)
}

fn usage_auth_error(stage: &str, status: reqwest::StatusCode) -> String {
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return "Claude session key is expired or invalid. Reconfigure usage tracking in Settings > Agent.".to_string();
    }

    format!("Claude {} request failed with HTTP {}", stage, status)
}

#[tauri::command]
pub fn agent_discover_sessions(project_path: String) -> Result<Vec<DiscoveredSession>, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let projects_dir = cli
        .session_dir_for_project(&project_path)
        .ok_or("Cannot determine home directory")?;

    if !projects_dir.exists() {
        return Ok(Vec::new());
    }

    let mut sessions = Vec::new();
    let entries = fs::read_dir(&projects_dir).map_err(|e| e.to_string())?;

    for entry in entries.flatten() {
        let path = entry.path();
        if cli.is_session_file(&path) {
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
            let preview = fs::read_to_string(&path).ok().and_then(|content| {
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
pub fn agent_get_session_tokens(
    project_path: String,
    session_id: Option<String>,
) -> Result<TokenUsage, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let projects_dir = cli
        .session_dir_for_project(&project_path)
        .ok_or("Cannot determine home directory")?;

    if !projects_dir.exists() {
        return Err("Project directory not found".to_string());
    }

    let file_path = if let Some(sid) = session_id {
        projects_dir.join(format!("{}.{}", sid, cli.session_file_extension()))
    } else {
        // Find most recent session file
        let mut best: Option<(PathBuf, std::time::SystemTime)> = None;
        if let Ok(entries) = fs::read_dir(&projects_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if cli.is_session_file(&path) {
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

    let contents = fs::read_to_string(&file_path).map_err(|e| e.to_string())?;

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

/// Get the context window usage for a session by reading the last assistant entry
/// from the .jsonl file. Returns fill percentage and compaction detection.
#[tauri::command]
pub fn agent_get_session_context_usage(
    project_path: String,
    session_id: String,
) -> Result<ContextUsage, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let file_path = cli
        .session_dir_for_project(&project_path)
        .ok_or("Cannot determine home directory")?
        .join(format!("{}.{}", session_id, cli.session_file_extension()));

    if !file_path.exists() {
        return Err("Session file not found".to_string());
    }

    // Read from the end for efficiency — find last two assistant entries
    let contents = fs::read_to_string(&file_path).map_err(|e| e.to_string())?;
    let lines: Vec<&str> = contents.lines().collect();

    let mut last_usage: Option<(u64, u64, u64, String)> = None; // (input, cache_read, cache_create, model)
    let mut prev_total: Option<u64> = None;
    let mut found_last = false;

    // Iterate from the end to find the last two assistant entries
    for line in lines.iter().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
            let is_assistant = val.get("type").and_then(|t| t.as_str()) == Some("assistant");
            if !is_assistant {
                continue;
            }

            let usage = val.get("message").and_then(|m| m.get("usage"));
            if let Some(u) = usage {
                let input = u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_read = u.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let cache_create = u.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
                let total = input + cache_read + cache_create;

                let model = val.get("message")
                    .and_then(|m| m.get("model"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                if !found_last {
                    last_usage = Some((input, cache_read, cache_create, model));
                    found_last = true;
                } else {
                    prev_total = Some(total);
                    break;
                }
            }
        }
    }

    let (input_tokens, cache_read_tokens, cache_creation_tokens, model) =
        last_usage.unwrap_or((0, 0, 0, "unknown".to_string()));

    let total_context_tokens = input_tokens + cache_read_tokens + cache_creation_tokens;

    // Determine context window size from model name
    let context_window: u64 = if model.contains("opus") {
        1_000_000
    } else if model.contains("sonnet") {
        200_000
    } else if model.contains("haiku") {
        200_000
    } else {
        200_000 // safe default
    };

    let fill_percent = if context_window > 0 {
        (total_context_tokens as f64 / context_window as f64) * 100.0
    } else {
        0.0
    };

    // Detect compaction: previous total was >50% higher than current
    let compacted = if let Some(prev) = prev_total {
        prev > 0 && total_context_tokens < prev / 2
    } else {
        false
    };

    Ok(ContextUsage {
        input_tokens,
        cache_read_tokens,
        cache_creation_tokens,
        total_context_tokens,
        context_window,
        fill_percent,
        model,
        compacted,
    })
}
