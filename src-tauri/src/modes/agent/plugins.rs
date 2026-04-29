use crate::modes::agent::models::{ClaudePlugin, MarketplacePlugin};
use crate::shared::cli::{claude::CLAUDE, runner::CliRunner};
use std::fs;

#[tauri::command]
pub fn agent_get_plugins() -> Result<Vec<ClaudePlugin>, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let settings_path = cli.settings_file().ok_or("Cannot determine home directory")?;
    let installed_path = cli.installed_plugins_file().ok_or("Cannot determine home directory")?;
    let mut enabled_map: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
    if settings_path.exists() {
        let content = fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        let settings: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if let Some(plugins) = settings.get("enabledPlugins").and_then(|v| v.as_object()) {
            for (key, val) in plugins { enabled_map.insert(key.clone(), val.as_bool().unwrap_or(false)); }
        }
    }
    let mut plugins = Vec::new();
    if installed_path.exists() {
        let content = fs::read_to_string(&installed_path).map_err(|e| e.to_string())?;
        let installed: serde_json::Value = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        if let Some(plugin_map) = installed.get("plugins").and_then(|v| v.as_object()) {
            for (key, entries) in plugin_map {
                let parts: Vec<&str> = key.splitn(2, '@').collect();
                let name = parts.first().unwrap_or(&"").to_string();
                let marketplace = parts.get(1).unwrap_or(&"").to_string();
                let (version, install_path) = entries.as_array().and_then(|arr| arr.first()).map(|entry| {
                    (entry.get("version").and_then(|v| v.as_str()).map(String::from), entry.get("installPath").and_then(|v| v.as_str()).map(String::from))
                }).unwrap_or((None, None));
                let enabled = enabled_map.get(key).copied().unwrap_or(false);
                plugins.push(ClaudePlugin { name, marketplace, enabled, version, install_path });
            }
        }
    }
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(plugins)
}

#[tauri::command]
pub fn agent_toggle_plugin(plugin_key: String, enabled: bool) -> Result<(), String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let settings_path = cli.settings_file().ok_or("Cannot determine home directory")?;
    let mut settings: serde_json::Value = if settings_path.exists() {
        serde_json::from_str(&fs::read_to_string(&settings_path).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?
    } else { serde_json::json!({}) };
    if settings.get("enabledPlugins").is_none() { settings["enabledPlugins"] = serde_json::json!({}); }
    settings["enabledPlugins"][&plugin_key] = serde_json::Value::Bool(enabled);
    fs::write(&settings_path, serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn agent_get_marketplace_plugins() -> Result<Vec<MarketplacePlugin>, String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let marketplaces_dir = cli.plugin_marketplaces_dir().ok_or("Cannot determine home directory")?;
    let installed_path = cli.installed_plugins_file().ok_or("Cannot determine home directory")?;
    let mut installed_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    if installed_path.exists() {
        if let Ok(content) = fs::read_to_string(&installed_path) {
            if let Ok(installed) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(plugins) = installed.get("plugins").and_then(|v| v.as_object()) {
                    for key in plugins.keys() { installed_keys.insert(key.clone()); }
                }
            }
        }
    }
    let mut install_counts: std::collections::HashMap<String, u64> = std::collections::HashMap::new();
    let counts_path = cli.plugin_install_counts_file().ok_or("Cannot determine home directory")?;
    if counts_path.exists() {
        if let Ok(content) = fs::read_to_string(&counts_path) {
            if let Ok(cache) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(counts) = cache.get("counts").and_then(|v| v.as_array()) {
                    for entry in counts {
                        if let (Some(plugin), Some(count)) = (entry.get("plugin").and_then(|v| v.as_str()), entry.get("unique_installs").and_then(|v| v.as_u64())) {
                            install_counts.insert(plugin.to_string(), count);
                        }
                    }
                }
            }
        }
    }
    let mut results = Vec::new();
    if !marketplaces_dir.exists() { return Ok(results); }
    for entry in fs::read_dir(&marketplaces_dir).map_err(|e| e.to_string())?.flatten() {
        let marketplace_name = entry.file_name().to_string_lossy().to_string();
        let registry_path = entry.path().join(".claude-plugin").join("marketplace.json");
        if !registry_path.exists() { continue; }
        let content = match fs::read_to_string(&registry_path) { Ok(c) => c, Err(_) => continue };
        let registry: serde_json::Value = match serde_json::from_str(&content) { Ok(v) => v, Err(_) => continue };
        if let Some(plugins) = registry.get("plugins").and_then(|v| v.as_array()) {
            for plugin in plugins {
                let name = plugin.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if name.is_empty() { continue; }
                let description = plugin.get("description").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let category = plugin.get("category").and_then(|v| v.as_str()).map(String::from);
                let key = format!("{}@{}", name, marketplace_name);
                results.push(MarketplacePlugin { name, description, marketplace: marketplace_name.clone(), category, installed: installed_keys.contains(&key), installs: install_counts.get(&key).copied() });
            }
        }
    }
    results.sort_by(|a, b| b.installs.unwrap_or(0).cmp(&a.installs.unwrap_or(0)));
    Ok(results)
}

#[tauri::command]
pub fn agent_install_plugin(name: String, marketplace: String) -> Result<(), String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let plugin_id = format!("{}@{}", name, marketplace);
    let (ok, stderr) = cli.run_plugin_subcommand(&["install", &plugin_id])?;
    if !ok { return Err(format!("Install failed: {}", stderr)); }
    Ok(())
}

#[tauri::command]
pub fn agent_uninstall_plugin(name: String, marketplace: String) -> Result<(), String> {
    let cli: &dyn CliRunner = &CLAUDE;
    let plugin_id = format!("{}@{}", name, marketplace);
    let (ok, stderr) = cli.run_plugin_subcommand(&["uninstall", &plugin_id])?;
    if !ok { return Err(format!("Uninstall failed: {}", stderr)); }
    Ok(())
}
