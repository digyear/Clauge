// SSH mode AI tool registration.
//
// SSH's only AI tool is `execute_shell`, which is bidirectional: the model
// requests it, the Rust chat loop emits `ai:tool_pending:<session>`, the
// frontend shows the approval modal + runs the command + redacts output,
// and `ai_resolve_pending_tool` unblocks the awaiting executor with the
// result. We preserve that flow exactly — the executor here is just the
// previous body of the `tool_name == "execute_shell"` early-return branch
// from `commands::ai::tools`.

use std::time::Duration;

use tauri::{Emitter, Manager};

use crate::commands::ai::types::PendingFrontendTools;
use crate::shared::ai::dispatch::{register, ToolContext, ToolDescriptor, ToolFuture};

fn execute_shell<'a>(ctx: &'a ToolContext<'a>) -> ToolFuture<'a> {
    Box::pin(async move {
        let app = ctx.app;
        let tool_use_id = ctx.tool_use_id;
        let session_id = ctx.session_id;
        let command = ctx
            .input
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let reason = ctx
            .input
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if command.is_empty() {
            return "Tool error: 'command' field is required".to_string();
        }
        let pending = app.state::<PendingFrontendTools>();
        let (tx, rx) = tokio::sync::oneshot::channel::<String>();
        pending.map.lock().insert(tool_use_id.to_string(), tx);
        let _ = app.emit(
            &format!("ai:tool_pending:{}", session_id),
            serde_json::json!({
                "toolUseId": tool_use_id,
                "tool": "execute_shell",
                "command": command,
                "reason": reason,
            }),
        );
        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => "Tool result channel closed unexpectedly.".to_string(),
            Err(_) => {
                app.state::<PendingFrontendTools>()
                    .map
                    .lock()
                    .remove(tool_use_id);
                "User did not respond to the approval prompt within 2 minutes.".to_string()
            }
        }
    })
}

/// Register the SSH-mode AI tool with the shared dispatch registry.
pub fn register_tools() {
    register(ToolDescriptor {
        name: "execute_shell",
        mode: "ssh",
        description: "Run a shell command on the remote SSH host (frontend captures the output and the user must approve).",
        schema: serde_json::json!({}),
        executor: execute_shell,
    });
}
