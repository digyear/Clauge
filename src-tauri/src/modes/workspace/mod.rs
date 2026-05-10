// Workspace mode — owns CRUD for Workspaces (containers), Notes
// (markdown pages), and Boards (Kanban with columns + cards).
//
// `commands` hosts `#[tauri::command]` handlers; `models` carries the
// shared data types. All persistence funnels through
// `crate::shared::repos::workspaces`.
//
// Agent integration (MCP server exposing notes/boards as tools) is
// architected here but registered separately when the workspace
// transport lands; for now `ai_tools::register_tools` is a no-op
// (mirrors agent mode's shape).

pub mod agent_spawn;
pub mod ai_tools;
pub mod cli_errors;
pub mod commands;
pub mod mcp;
pub mod models;
pub mod pr;
pub mod push;
