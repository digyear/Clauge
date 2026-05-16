// Cloudflare D1 HTTP client.
//
// D1 is Cloudflare's serverless SQLite-flavoured database. It has no native
// wire protocol — every query is a POST to the Cloudflare API. So like
// ClickHouse, it lives outside the sqlx pool model: this module wraps a
// `reqwest::Client`, an account-id + database-id pair, and an API token.
//
// API endpoint (v4):
//   POST https://api.cloudflare.com/client/v4/accounts/{account}/d1/database/{db}/query
//   Authorization: Bearer <token>
//   Content-Type: application/json
//   Body: { "sql": "...", "params": [] }
//
// Response shape:
//   {
//     "success": true,
//     "errors": [{ "code": 7501, "message": "..." }, ...],
//     "messages": [...],
//     "result": [
//       {
//         "results": [{ "col1": ..., "col2": ... }, ...],
//         "success": true,
//         "meta": {
//           "duration": 0.07,            // seconds
//           "changes": 0,
//           "rows_read": 1,
//           "rows_written": 0,
//           "last_row_id": null,
//           "size_after": 8192,
//           "served_by": "v3-prod"
//         }
//       }
//     ]
//   }
//
// Saved-connection field reuse (matches the ClickHouse pattern of stuffing
// HTTP-driver config into the `SqlConnectionConfig` shape):
//   host     → account_id
//   database → database_id
//   password → api_token (Cloudflare API token with `D1:Edit` scope)
//   username / port / ssl: unused (D1 is always HTTPS to api.cloudflare.com)

use serde::Deserialize;
use std::time::Duration;

use super::client::SqlConnectionConfig;

/// Map the most common Cloudflare D1 / API error codes to a single-line
/// toast string. Unmapped codes fall through to `clean_d1_error_body`
/// which extracts the first descriptive message from the errors array.
fn humanize_d1_code(code: i64) -> Option<&'static str> {
    match code {
        // Authentication / authorisation.
        // 10000 = generic unauthorised (used when token is invalid).
        // 9109 / 6003 = "Invalid request headers" — usually missing/malformed Authorization.
        10000 | 9109 | 6003 => Some(
            "Authentication failed — check the API token and that it has D1:Edit permission",
        ),
        // D1-specific.
        7400 => Some("Bad SQL — D1 rejected the statement"),
        7404 => Some("Database not found — check the Database ID"),
        7500 => Some("D1 internal error — retry in a moment"),
        7501 => Some("Rate-limited by Cloudflare — wait and retry"),
        7503 => Some("Response too large — D1 caps results at ~100 KB"),
        // Cloudflare generic.
        9106 => Some("Account not found — check the Account ID"),
        _ => None,
    }
}

/// Pull a short, single-line message out of the D1 errors array. Used when
/// `humanize_d1_code` doesn't have a mapping for the failing code.
fn clean_d1_error_body(errors: &[D1ApiError]) -> String {
    let mut out = String::new();
    for (i, e) in errors.iter().take(3).enumerate() {
        if i > 0 {
            out.push_str("; ");
        }
        out.push_str(e.message.trim());
        if e.code != 0 {
            out.push_str(&format!(" (code {})", e.code));
        }
    }
    if out.is_empty() {
        out.push_str("Unknown D1 error");
    }
    if out.len() > 280 {
        out.truncate(280);
        out.push('…');
    }
    out
}

/// Stateless HTTP client for a single Cloudflare D1 database. Cloning is
/// cheap; `reqwest::Client` shares its connection pool internally.
#[derive(Debug, Clone)]
pub struct D1Client {
    http: reqwest::Client,
    /// `https://api.cloudflare.com/client/v4/accounts/{acct}/d1/database/{db}/query`
    query_url: String,
    api_token: String,
    /// Stable identifiers — exposed so list-databases / list-schemas can
    /// surface the database id without re-parsing the URL.
    pub database: String,
    pub account_id: String,
}

/// Top-level wire shape.
#[derive(Debug, Deserialize)]
struct D1ApiResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    errors: Vec<D1ApiError>,
    #[serde(default)]
    result: Vec<D1ResultEntry>,
}

#[derive(Debug, Deserialize)]
struct D1ApiError {
    #[serde(default)]
    code: i64,
    #[serde(default)]
    message: String,
}

#[derive(Debug, Deserialize)]
struct D1ResultEntry {
    #[serde(default)]
    results: Vec<serde_json::Map<String, serde_json::Value>>,
    #[serde(default)]
    meta: D1Meta,
}

#[derive(Debug, Default, Deserialize)]
struct D1Meta {
    /// Number of rows changed by an INSERT/UPDATE/DELETE. Zero for SELECTs.
    #[serde(default)]
    changes: u64,
    /// Wall-clock duration in seconds (float). Converted to ms by the caller.
    #[serde(default)]
    duration: f64,
}

/// Parsed query result in the same shape `sql_execute_query` returns for
/// the other drivers (column names + JSON-typed row values + affected
/// count + ms).
pub struct D1QueryRows {
    pub columns: Vec<String>,
    pub rows: Vec<Vec<serde_json::Value>>,
    pub affected: u64,
    pub duration_ms: u64,
}

impl D1Client {
    /// Build a client from a `SqlConnectionConfig` whose fields have been
    /// stuffed per the field-reuse mapping documented at the top of this
    /// file (`host`=account_id, `database`=database_id, `password`=token).
    ///
    /// `app_pool` is the Clauge SQLite pool (used for the shared HTTP
    /// proxy settings). `None` falls back to a vanilla `reqwest::Client`.
    pub async fn new(
        config: &SqlConnectionConfig,
        app_pool: Option<&sqlx::SqlitePool>,
    ) -> Result<Self, String> {
        let account_id = config.host.trim();
        let database_id = config.database.trim();
        let api_token = config.password.trim();
        if account_id.is_empty() {
            return Err("D1: Account ID is required".to_string());
        }
        if database_id.is_empty() {
            return Err("D1: Database ID is required".to_string());
        }
        if api_token.is_empty() {
            return Err("D1: API token is required (create one at https://dash.cloudflare.com/profile/api-tokens with the D1:Edit permission)".to_string());
        }

        let query_url = format!(
            "https://api.cloudflare.com/client/v4/accounts/{}/d1/database/{}/query",
            account_id, database_id
        );

        let http = if let Some(pool) = app_pool {
            crate::shared::http::build_sql_http_client(pool).await?
        } else {
            reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .map_err(|e| format!("D1 client build failed: {}", e))?
        };

        Ok(Self {
            http,
            query_url,
            api_token: api_token.to_string(),
            database: database_id.to_string(),
            account_id: account_id.to_string(),
        })
    }

    /// Verify reachability + credentials with a `SELECT 1`.
    pub async fn ping(&self) -> Result<(), String> {
        let _ = self.query("SELECT 1").await?;
        Ok(())
    }

    /// Execute a single SQL statement and return parsed rows + meta.
    ///
    /// D1 supports semicolon-separated batches and returns one result per
    /// statement in `result[]`. We pick `result[0]` so multi-statement
    /// behaviour matches the other drivers (run-the-first, ignore the rest).
    /// Users who need true batching can write a single statement.
    pub async fn query(&self, sql: &str) -> Result<D1QueryRows, String> {
        let body = serde_json::json!({
            "sql": sql,
        });

        let response = self
            .http
            .post(&self.query_url)
            .header("Authorization", format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("D1 request failed: {}", e))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| format!("D1 response read failed: {}", e))?;

        // The API returns 200 even for SQL errors — `success: false` in the
        // body is the signal. Non-2xx is reserved for transport / auth /
        // platform errors, which still come with the same JSON envelope.
        let parsed: D1ApiResponse = serde_json::from_str(&text).map_err(|e| {
            format!(
                "D1 JSON parse failed (HTTP {}): {} (body: {})",
                status,
                e,
                text.chars().take(400).collect::<String>()
            )
        })?;

        if !parsed.success {
            // Prefer the per-code hand-written message; fall back to the
            // first descriptive message in the errors array.
            let first_code = parsed.errors.first().map(|e| e.code).unwrap_or(0);
            let msg = match humanize_d1_code(first_code) {
                Some(m) => m.to_string(),
                None => clean_d1_error_body(&parsed.errors),
            };
            return Err(msg);
        }

        let entry = parsed.result.into_iter().next().unwrap_or(D1ResultEntry {
            results: Vec::new(),
            meta: D1Meta::default(),
        });

        // Column order isn't returned by the API; we derive it from the
        // first row's key order. SQLite's JSON serialiser preserves the
        // SELECT-list order, so this matches what the user wrote.
        let columns: Vec<String> = entry
            .results
            .first()
            .map(|row| row.keys().cloned().collect())
            .unwrap_or_default();

        let rows: Vec<Vec<serde_json::Value>> = entry
            .results
            .iter()
            .map(|row| {
                columns
                    .iter()
                    .map(|c| row.get(c).cloned().unwrap_or(serde_json::Value::Null))
                    .collect()
            })
            .collect();

        // For SELECT, surface row count as the "affected" so the UI prints
        // a sensible "N rows" — matches the other drivers. For mutations,
        // D1's `changes` is the actual mutated-row count.
        let affected = if entry.meta.changes > 0 {
            entry.meta.changes
        } else {
            rows.len() as u64
        };
        let duration_ms = (entry.meta.duration * 1000.0).round() as u64;

        Ok(D1QueryRows {
            columns,
            rows,
            affected,
            duration_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_known_codes() {
        assert!(humanize_d1_code(10000).is_some());
        assert!(humanize_d1_code(7404).is_some());
        assert!(humanize_d1_code(7501).is_some());
        assert!(humanize_d1_code(9999).is_none());
    }

    #[test]
    fn clean_error_body_truncates_long() {
        let errs = vec![D1ApiError {
            code: 7400,
            message: "x".repeat(500),
        }];
        let cleaned = clean_d1_error_body(&errs);
        assert!(cleaned.len() <= 281);
        assert!(cleaned.contains("(code 7400)"));
    }

    #[test]
    fn clean_error_body_joins_first_three() {
        let errs = vec![
            D1ApiError { code: 1, message: "first".into() },
            D1ApiError { code: 2, message: "second".into() },
            D1ApiError { code: 3, message: "third".into() },
            D1ApiError { code: 4, message: "fourth".into() },
        ];
        let cleaned = clean_d1_error_body(&errs);
        assert!(cleaned.contains("first"));
        assert!(cleaned.contains("second"));
        assert!(cleaned.contains("third"));
        assert!(!cleaned.contains("fourth"));
    }
}
