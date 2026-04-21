use std::path::PathBuf;
use crate::storage::{load_profiles, save_profiles};

/// Check if a path is inside a git repo
#[tauri::command]
pub fn is_git_repo(path: String) -> Result<bool, String> {
    let output = std::process::Command::new("git")
        .args(["-C", &path, "rev-parse", "--is-inside-work-tree"])
        .output()
        .map_err(|e| e.to_string())?;
    Ok(output.status.success())
}

/// Sanitize a branch name to comply with git naming rules.
/// Removes dangerous characters, prevents flag injection (leading `-`),
/// and enforces git's naming constraints.
fn sanitize_branch_name(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '/' || *c == '-' || *c == '_' || *c == '.')
        .collect();

    // Collapse consecutive dots/dashes, remove trailing dots/locks
    let sanitized = sanitized
        .replace("..", ".")
        .replace(".lock", "")
        .trim_matches(|c: char| c == '.' || c == '/' || c == '-')
        .to_string();

    if sanitized.is_empty() {
        return "clauge/unnamed".to_string();
    }

    // Prevent flag injection: if any path segment starts with `-`, prefix it
    sanitized
        .split('/')
        .map(|seg| {
            if seg.starts_with('-') {
                format!("x{}", seg)
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Create a git worktree for session isolation
#[tauri::command]
pub fn create_worktree(project_path: String, branch_name: String) -> Result<String, String> {
    let branch_name = sanitize_branch_name(&branch_name);
    let worktree_dir = PathBuf::from(&project_path)
        .join(".clauge-worktrees")
        .join(&branch_name);
    let worktree_path = worktree_dir.to_string_lossy().to_string();

    // If worktree directory already exists and is valid, just reuse it
    if worktree_dir.exists() {
        return Ok(worktree_path);
    }

    let _ = std::fs::create_dir_all(worktree_dir.parent().unwrap_or(&worktree_dir));

    // Prune stale worktrees first
    let _ = std::process::Command::new("git")
        .args(["-C", &project_path, "worktree", "prune"])
        .output();

    // Try creating with new branch
    let output = std::process::Command::new("git")
        .args(["-C", &project_path, "worktree", "add", "-b", &branch_name, &worktree_path])
        .output()
        .map_err(|e| format!("git worktree add failed: {}", e))?;

    if !output.status.success() {
        // Branch exists — reuse it
        let output2 = std::process::Command::new("git")
            .args(["-C", &project_path, "worktree", "add", &worktree_path, &branch_name])
            .output()
            .map_err(|e| format!("git worktree add failed: {}", e))?;

        if !output2.status.success() {
            return Err(format!("git worktree add failed: {}", String::from_utf8_lossy(&output2.stderr)));
        }
    }

    // Add .clauge-worktrees to .gitignore if not already there
    let gitignore = PathBuf::from(&project_path).join(".gitignore");
    if let Ok(contents) = std::fs::read_to_string(&gitignore) {
        if !contents.contains(".clauge-worktrees") {
            let _ = std::fs::write(&gitignore, format!("{}\n.clauge-worktrees/\n", contents.trim_end()));
        }
    } else {
        let _ = std::fs::write(&gitignore, ".clauge-worktrees/\n");
    }

    Ok(worktree_path)
}

/// Remove a git worktree (keeps the branch — user may have commits there)
#[tauri::command]
pub fn remove_worktree(project_path: String, worktree_path: String, _branch_name: Option<String>) -> Result<(), String> {
    let _ = std::process::Command::new("git")
        .args(["-C", &project_path, "worktree", "remove", "--force", &worktree_path])
        .output();

    let _ = std::process::Command::new("git")
        .args(["-C", &project_path, "worktree", "prune"])
        .output();

    Ok(())
}

/// Update worktree info for a profile
#[tauri::command]
pub fn update_profile_worktree(id: String, worktree_path: Option<String>, worktree_branch: Option<String>) -> Result<(), String> {
    let mut profiles = load_profiles();
    if let Some(profile) = profiles.iter_mut().find(|p| p.id == id) {
        profile.worktree_path = worktree_path;
        profile.worktree_branch = worktree_branch;
    } else {
        return Err("Profile not found".to_string());
    }
    save_profiles(&profiles)?;
    Ok(())
}
