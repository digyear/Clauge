// Agent mode AI tool registration.
//
// Agent mode currently exposes no model-callable tools through the AI chat
// dispatch; the Claude Code CLI it shells out to handles its own tool use
// internally. We still expose `register_tools()` to keep the shape uniform
// across modes so future agent-side tools register exactly like SQL/NoSQL/REST.

/// Register every Agent-mode AI tool with the shared dispatch registry.
/// Currently a no-op. Add new descriptors here when the mode grows tools.
pub fn register_tools() {
    // No agent-mode AI tools yet.
}
