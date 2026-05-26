// HTTP / JSON-RPC transport. Owns the bind + serve loop and the
// single `POST /mcp` request handler that fans out to dispatch.

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Json as JsonResponse},
    routing::post,
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::{actor::actor_from_request, dispatch::dispatch_tool, tools::tool_descriptors};

const PROTOCOL_VERSION: &str = "2025-06-18";

/// How many sequential ports `start` will try if the requested one is
/// already in use. Six attempts (requested + 5) covers the common
/// "another dev tool is on 7421" case without giving up too quickly,
/// while staying small enough that a totally-blocked range still
/// fails fast.
const PORT_FALLBACK_RANGE: u16 = 5;

pub struct McpHandle {
    pub port: u16,
    pub shutdown: Option<oneshot::Sender<()>>,
}

#[derive(Clone)]
pub(super) struct McpAppState {
    pub(super) pool: SqlitePool,
    pub(super) token: String,
    /// Optional Tauri handle so MCP-side mutations can emit live
    /// events (e.g. `workspace:card-updated`). None when MCP runs
    /// before the Tauri app is fully bootstrapped — emits become
    /// no-ops in that window.
    pub(super) app: Option<tauri::AppHandle>,
}

/// Bind a listener on 127.0.0.1, spawn the axum server, and return a
/// handle whose `shutdown` sender stops the server. Tries
/// `requested_port` first; on `AddrInUse` walks up to
/// `requested_port + PORT_FALLBACK_RANGE`. The handle's `port` field
/// is the port that was actually bound — caller should compare
/// against the requested port and persist / re-register if it
/// differs.
pub async fn start(
    pool: SqlitePool,
    requested_port: u16,
    token: String,
    app: Option<tauri::AppHandle>,
) -> Result<McpHandle, String> {
    let mut last_err: Option<String> = None;
    for offset in 0..=PORT_FALLBACK_RANGE {
        let port = match requested_port.checked_add(offset) {
            Some(p) => p,
            None => break, // overflowed past u16::MAX
        };
        let addr = format!("127.0.0.1:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                let state = McpAppState { pool, token, app };
                let router = Router::new()
                    .route("/mcp", post(handle_mcp))
                    .route("/healthz", axum::routing::get(|| async { "ok" }))
                    .with_state(Arc::new(state));

                let (tx, rx) = oneshot::channel::<()>();
                tokio::spawn(async move {
                    let _ = axum::serve(listener, router)
                        .with_graceful_shutdown(async {
                            let _ = rx.await;
                        })
                        .await;
                });
                return Ok(McpHandle {
                    port,
                    shutdown: Some(tx),
                });
            }
            Err(e) => {
                last_err = Some(format!("{}: {}", addr, e));
            }
        }
    }
    Err(format!(
        "Failed to bind any port in {}..={}: {}",
        requested_port,
        requested_port.saturating_add(PORT_FALLBACK_RANGE),
        last_err.unwrap_or_default(),
    ))
}

/// Single JSON-RPC POST handler. We dispatch on `method` and respond
/// with either `result` or `error` — never both.
async fn handle_mcp(
    State(state): State<Arc<McpAppState>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    // Bearer auth — strict comparison, no fallthrough.
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    if token != state.token {
        return (
            StatusCode::UNAUTHORIZED,
            JsonResponse(json!({
                "jsonrpc": "2.0",
                "error": { "code": -32001, "message": "Unauthorized" }
            })),
        )
            .into_response();
    }

    let method = body.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let params = body.get("params").cloned().unwrap_or(json!({}));
    let has_id = body
        .as_object()
        .map(|m| m.contains_key("id"))
        .unwrap_or(false);
    let is_client_response =
        method.is_empty() && (body.get("result").is_some() || body.get("error").is_some());

    // Streamable HTTP requires JSON-RPC notifications and client
    // responses to return 202 Accepted with no body.
    // Returning a JSON-RPC response here makes stricter clients
    // (Codex/rmcp) close the transport during the initialized
    // notification.
    if !has_id || is_client_response {
        match method {
            "notifications/initialized" => {}
            other => {
                log::debug!(
                    target: "workspace::mcp",
                    "accepted MCP notification without response: {other}"
                );
            }
        }
        return StatusCode::ACCEPTED.into_response();
    }

    let id = body.get("id").cloned().unwrap_or(Value::Null);

    // Resolve the actor for THIS request. Mutating tools route this
    // string straight into `updated_by`, so attribution + Inbox work
    // identically across Claude / Codex / Gemini / etc.
    let actor = actor_from_request(&headers, &body);

    let result: Result<Value, (i32, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": "clauge-workspace", "version": "1.0.0" }
        })),
        "tools/list" => Ok(json!({ "tools": tool_descriptors() })),
        "tools/call" => dispatch_tool(&state.pool, state.app.as_ref(), params, &actor).await,
        "ping" => Ok(json!({})),
        _ => Err((-32601, format!("Method not found: {}", method))),
    };

    let response = match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, msg)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": msg } })
        }
    };
    (StatusCode::OK, JsonResponse(response)).into_response()
}
