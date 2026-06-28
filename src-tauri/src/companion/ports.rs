// /v1/ports — listening TCP ports with owning process, for the mobile
// Ports inspector. /v1/proxy/{port}/* — a best-effort HTTP reverse proxy
// to 127.0.0.1:{port} so the phone can open local dev servers. The proxy
// forwards method/headers/body and relays the response; it is bearer-authed
// like the rest of /v1, so the embedded WebView must attach the token on
// every request (it intercepts and re-issues through the host). WebSocket
// upgrades are not proxied.

use std::collections::HashSet;

use axum::{
    body::{Body, Bytes},
    extract::{Path, RawQuery},
    http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode},
    response::{IntoResponse, Json, Response},
};
use netstat2::{
    get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState,
};
use serde::Serialize;
use serde_json::{json, Value};
use sysinfo::{Pid, ProcessesToUpdate, System};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PortInfo {
    port: u16,
    bind_addr: String,
    pid: Option<u32>,
    process: Option<String>,
}

pub async fn list_ports() -> Json<Value> {
    Json(json!({ "ports": collect_ports() }))
}

fn collect_ports() -> Vec<PortInfo> {
    let af = AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6;
    let sockets = match get_sockets_info(af, ProtocolFlags::TCP) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let mut out: Vec<PortInfo> = Vec::new();
    let mut seen: HashSet<(u16, Option<u32>)> = HashSet::new();
    for si in sockets {
        let ProtocolSocketInfo::Tcp(tcp) = si.protocol_socket_info else { continue };
        if tcp.state != TcpState::Listen {
            continue;
        }
        let pid = si.associated_pids.first().copied();
        if !seen.insert((tcp.local_port, pid)) {
            continue;
        }
        let process = pid
            .and_then(|p| sys.process(Pid::from_u32(p)))
            .map(|pr| pr.name().to_string_lossy().to_string());
        out.push(PortInfo {
            port: tcp.local_port,
            bind_addr: tcp.local_addr.to_string(),
            pid,
            process,
        });
    }
    out.sort_by(|a, b| a.port.cmp(&b.port));
    out
}

// -- Reverse proxy ----------------------------------------------------------

pub async fn proxy_root(
    Path(port): Path<u16>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward(port, String::new(), query, method, headers, body).await
}

pub async fn proxy_path(
    Path((port, path)): Path<(u16, String)>,
    RawQuery(query): RawQuery,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    forward(port, path, query, method, headers, body).await
}

async fn forward(
    port: u16,
    path: String,
    query: Option<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let q = query
        .filter(|s| !s.is_empty())
        .map(|s| format!("?{s}"))
        .unwrap_or_default();
    let url = format!("http://127.0.0.1:{port}/{path}{q}");
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("proxy client build failed: {e}") })),
            )
                .into_response()
        }
    };

    let req_method = reqwest::Method::from_bytes(method.as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);
    let mut rb = client.request(req_method, &url);
    for (name, value) in headers.iter() {
        // Host/Connection are hop-by-hop. Authorization carries the companion's
        // own bearer token — never forward it to the proxied localhost app.
        let n = name.as_str();
        if n.eq_ignore_ascii_case("host")
            || n.eq_ignore_ascii_case("connection")
            || n.eq_ignore_ascii_case("authorization")
        {
            continue;
        }
        rb = rb.header(n, value.as_bytes());
    }
    if !body.is_empty() {
        rb = rb.body(body.to_vec());
    }

    let resp = match rb.send().await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("proxy to :{port} failed: {e}") })),
            )
                .into_response()
        }
    };

    let status = StatusCode::from_u16(resp.status().as_u16()).unwrap_or(StatusCode::OK);
    let mut out_headers = HeaderMap::new();
    for (name, value) in resp.headers().iter() {
        let n = name.as_str();
        // These are recomputed for the relayed body.
        if n.eq_ignore_ascii_case("content-length")
            || n.eq_ignore_ascii_case("transfer-encoding")
            || n.eq_ignore_ascii_case("connection")
        {
            continue;
        }
        if let (Ok(hn), Ok(hv)) = (
            HeaderName::from_bytes(n.as_bytes()),
            HeaderValue::from_bytes(value.as_bytes()),
        ) {
            out_headers.insert(hn, hv);
        }
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({ "error": format!("proxy body read failed: {e}") })),
            )
                .into_response()
        }
    };

    (status, out_headers, Body::from(bytes)).into_response()
}
