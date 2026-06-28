// /v1/fs/* — host filesystem access for the mobile Files browser: list,
// read (text), write, mkdir, delete, download (bytes), upload (bytes),
// and a bounded recursive name search. Paths are absolute on the host;
// an absent path defaults to the home directory. All authed via the /v1
// bearer middleware. camelCase shapes per the mobile spec.

use std::path::{Path, PathBuf};

use axum::{
    body::Bytes,
    extract::Query,
    http::{header, StatusCode},
    response::{IntoResponse, Json as JsonResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

/// Largest file we'll return inline to the text viewer (2 MB); larger files
/// are reported as such so the phone offers download instead.
const MAX_READ: u64 = 2_000_000;
/// Cap on a single download so a huge file can't be buffered into memory and
/// OOM the companion server.
const MAX_DOWNLOAD: u64 = 100_000_000;
const SEARCH_MAX_RESULTS: usize = 200;
const SEARCH_MAX_DEPTH: usize = 8;
/// Hard ceiling on directory entries a single search may visit, so a search
/// rooted at a huge tree can't run unbounded.
const SEARCH_MAX_VISITED: usize = 50_000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    name: String,
    path: String,
    is_dir: bool,
    size: u64,
}

fn err(status: StatusCode, msg: &str) -> Response {
    (status, JsonResponse(json!({ "error": msg }))).into_response()
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Reject non-absolute paths: every fs handler documents absolute host paths,
/// and a relative path would resolve against the server's working directory.
fn require_abs(path: &str) -> Result<PathBuf, Response> {
    let p = PathBuf::from(path);
    if p.is_absolute() {
        Ok(p)
    } else {
        Err(err(StatusCode::BAD_REQUEST, "path must be absolute"))
    }
}

/// An explicit non-empty (absolute) path wins; otherwise the home directory.
fn resolve_checked(path: &Option<String>) -> Result<PathBuf, Response> {
    match path {
        Some(p) if !p.trim().is_empty() => require_abs(p),
        _ => Ok(home_dir()),
    }
}

/// A safe upload filename: exactly one normal path component (no separators,
/// no `.`/`..`) so an upload can't escape its destination directory.
fn safe_filename(name: &str) -> Option<String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut comps = Path::new(trimmed).components();
    match (comps.next(), comps.next()) {
        (Some(std::path::Component::Normal(c)), None) => Some(c.to_string_lossy().to_string()),
        _ => None,
    }
}

// -- GET /v1/fs/list --------------------------------------------------------

#[derive(Deserialize)]
pub struct ListQuery {
    path: Option<String>,
    hidden: Option<bool>,
}

pub async fn list(Query(q): Query<ListQuery>) -> Response {
    let dir = match resolve_checked(&q.path) {
        Ok(d) => d,
        Err(r) => return r,
    };
    let show_hidden = q.hidden.unwrap_or(false);

    let read = match std::fs::read_dir(&dir) {
        Ok(rd) => rd,
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("cannot read directory: {e}")),
    };

    let mut entries: Vec<Entry> = Vec::new();
    for e in read.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !show_hidden && name.starts_with('.') {
            continue;
        }
        let meta = e.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        entries.push(Entry {
            name,
            path: e.path().to_string_lossy().to_string(),
            is_dir,
            size,
        });
    }
    // Directories first, then case-insensitive by name.
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    JsonResponse(json!({
        "path": dir.to_string_lossy(),
        "parent": dir.parent().map(|p| p.to_string_lossy().to_string()),
        "entries": entries,
    }))
    .into_response()
}

// -- GET /v1/fs/read --------------------------------------------------------

#[derive(Deserialize)]
pub struct PathQuery {
    path: String,
}

pub async fn read(Query(q): Query<PathQuery>) -> Response {
    let path = match require_abs(&q.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("not found: {e}")),
    };
    if meta.is_dir() {
        return err(StatusCode::BAD_REQUEST, "path is a directory");
    }
    if meta.len() > MAX_READ {
        return JsonResponse(json!({
            "path": q.path, "binary": false, "tooLarge": true, "size": meta.len(),
        }))
        .into_response();
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, &format!("read failed: {e}")),
    };
    match String::from_utf8(bytes) {
        Ok(content) => JsonResponse(json!({
            "path": q.path, "binary": false, "tooLarge": false, "content": content,
        }))
        .into_response(),
        Err(_) => JsonResponse(json!({ "path": q.path, "binary": true })).into_response(),
    }
}

// -- GET /v1/fs/download ----------------------------------------------------

pub async fn download(Query(q): Query<PathQuery>) -> Response {
    let path = match require_abs(&q.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match std::fs::metadata(&path) {
        Ok(m) if m.len() > MAX_DOWNLOAD => {
            return err(StatusCode::PAYLOAD_TOO_LARGE, "file too large to download");
        }
        Ok(_) => {}
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("not found: {e}")),
    }
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("not found: {e}")),
    };
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "download".to_string());
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
        ],
        bytes,
    )
        .into_response()
}

// -- POST /v1/fs/mkdir | /v1/fs/write ---------------------------------------

#[derive(Deserialize)]
pub struct MkdirBody {
    path: String,
}

pub async fn mkdir(JsonResponse(b): JsonResponse<MkdirBody>) -> Response {
    if let Err(r) = require_abs(&b.path) {
        return r;
    }
    match std::fs::create_dir_all(&b.path) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("mkdir failed: {e}")),
    }
}

#[derive(Deserialize)]
pub struct WriteBody {
    path: String,
    content: String,
}

pub async fn write(JsonResponse(b): JsonResponse<WriteBody>) -> Response {
    if let Err(r) = require_abs(&b.path) {
        return r;
    }
    if let Some(parent) = Path::new(&b.path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(&b.path, b.content.as_bytes()) {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("write failed: {e}")),
    }
}

// -- POST /v1/fs/upload?path=<dir>&name=<file> (raw body) -------------------

#[derive(Deserialize)]
pub struct UploadQuery {
    path: String,
    name: String,
}

pub async fn upload(Query(q): Query<UploadQuery>, body: Bytes) -> Response {
    let dir = match require_abs(&q.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let Some(name) = safe_filename(&q.name) else {
        return err(StatusCode::BAD_REQUEST, "invalid file name");
    };
    let dest = dir.join(name);
    match std::fs::write(&dest, &body) {
        Ok(_) => JsonResponse(json!({ "path": dest.to_string_lossy() })).into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("upload failed: {e}")),
    }
}

// -- DELETE /v1/fs/delete?path=<> -------------------------------------------

pub async fn delete(Query(q): Query<PathQuery>) -> Response {
    let path = match require_abs(&q.path) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) => return err(StatusCode::NOT_FOUND, &format!("not found: {e}")),
    };
    let result = if meta.is_dir() {
        std::fs::remove_dir_all(&path)
    } else {
        std::fs::remove_file(&path)
    };
    match result {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, &format!("delete failed: {e}")),
    }
}

// -- GET /v1/fs/search?path=<>&q=<> -----------------------------------------

#[derive(Deserialize)]
pub struct SearchQuery {
    path: Option<String>,
    q: String,
}

pub async fn search(Query(query): Query<SearchQuery>) -> Response {
    let root = match resolve_checked(&query.path) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let needle = query.q.trim().to_lowercase();
    if needle.is_empty() {
        return JsonResponse(json!({ "entries": Vec::<Entry>::new() })).into_response();
    }
    // The walk is blocking and potentially large — run it off the async worker
    // and bound how many entries it may visit.
    let results = tokio::task::spawn_blocking(move || {
        let mut results: Vec<Entry> = Vec::new();
        let mut budget = SEARCH_MAX_VISITED;
        walk(&root, &needle, 0, &mut results, &mut budget);
        results
    })
    .await
    .unwrap_or_default();
    JsonResponse(json!({ "entries": results })).into_response()
}

fn walk(dir: &Path, needle: &str, depth: usize, out: &mut Vec<Entry>, budget: &mut usize) {
    if depth > SEARCH_MAX_DEPTH || out.len() >= SEARCH_MAX_RESULTS || *budget == 0 {
        return;
    }
    let Ok(read) = std::fs::read_dir(dir) else { return };
    for e in read.flatten() {
        if out.len() >= SEARCH_MAX_RESULTS || *budget == 0 {
            return;
        }
        *budget -= 1;
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let meta = e.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        if name.to_lowercase().contains(needle) {
            out.push(Entry {
                name: name.clone(),
                path: e.path().to_string_lossy().to_string(),
                is_dir,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
            });
        }
        if is_dir {
            walk(&e.path(), needle, depth + 1, out, budget);
        }
    }
}
