// Workspace mode AI tool registration.
//
// The Workspace data model (Notes + Boards) is meant to be fully
// readable/writable by AI workers via tool calls. The transport layer
// (MCP server for Claude Code; future Codex / Gemini adapters) lands
// in a later wave. This module is the registration point so the rest
// of the code can import a stable symbol.

/// Register every Workspace-mode AI tool with the shared dispatch
/// registry. Currently a no-op — wire up MCP/etc. tools here.
pub fn register_tools() {
    // No tools registered yet.
}
