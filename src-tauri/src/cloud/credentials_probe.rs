// Detects connections whose credentials weren't restored by cloud sync.
// Cloud export deliberately strips passwords + omits keychain entries
// (per-device by design), so a fresh-device restore leaves every saved
// connection looking intact but unable to actually connect. This probe
// walks each mode's rows once and returns the ids that are missing the
// secret they'd need to authenticate. The frontend renders a "Sign-in
// needed" affordance and short-circuits connect attempts with a friendly
// toast instead of letting the user puzzle over an auth error.

use serde::Serialize;
use sqlx::SqlitePool;

use crate::shared::platform::credential_store::{credential_store, CredentialStore};

#[derive(Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MissingCredentials {
    pub ssh: Vec<String>,
    pub sql: Vec<String>,
    pub nosql: Vec<String>,
    pub explorer: Vec<String>,
}

#[tauri::command]
pub async fn cloud_probe_missing_credentials(
    pool: tauri::State<'_, SqlitePool>,
) -> Result<MissingCredentials, String> {
    let pool = pool.inner();
    let store = credential_store();
    let mut out = MissingCredentials::default();

    // ─── SQL ───────────────────────────────────────────────────────────────
    // Password lives in the row itself (not keychain). Cloud export strips
    // it, so restore inserts the default empty string → flag.
    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT id, password FROM sql_connections")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("probe sql: {}", e))?;
    for (id, pw) in rows {
        if pw.unwrap_or_default().is_empty() {
            out.sql.push(id);
        }
    }

    // ─── NoSQL ─────────────────────────────────────────────────────────────
    // Either password OR connection_string is enough — Mongo SRV / Redis
    // users typically paste a full URL into `connection_string` instead of
    // splitting username + password.
    let rows: Vec<(String, Option<String>, Option<String>)> = sqlx::query_as(
        "SELECT id, password, connection_string FROM nosql_connections",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| format!("probe nosql: {}", e))?;
    for (id, pw, cs) in rows {
        let has_pw = !pw.unwrap_or_default().is_empty();
        let has_cs = !cs.unwrap_or_default().is_empty();
        if !has_pw && !has_cs {
            out.nosql.push(id);
        }
    }

    // ─── SSH ───────────────────────────────────────────────────────────────
    // Per-row keychain entry under the bare profile id (service
    // "ZeroAny Workbench SSH"). Three auth shapes:
    //   - password: keychain miss → flag
    //   - key:      key_path missing on disk → flag (passphrase, if any,
    //               is optional; we don't second-guess it)
    //   - agent:    no secret needed → skip
    let rows: Vec<(String, String, Option<String>)> =
        sqlx::query_as("SELECT id, auth_type, key_path FROM ssh_profiles")
            .fetch_all(pool)
            .await
            .map_err(|e| format!("probe ssh: {}", e))?;
    for (id, auth_type, key_path) in rows {
        let missing = match auth_type.as_str() {
            "password" => !has_secret(&store, &id).await,
            "key" => match key_path {
                Some(p) if !p.trim().is_empty() => !std::path::Path::new(&p).exists(),
                _ => true,
            },
            _ => false,
        };
        if missing {
            out.ssh.push(id);
        }
    }

    // ─── Explorer ──────────────────────────────────────────────────────────
    // Per-kind secret namespace under "explorer:<id>:<secret_name>" in the
    // shared keychain service. SFTP via an `ssh_profile_id` defers entirely
    // to SSH (those ids already flagged above), so we only check the direct
    // shape here.
    let rows: Vec<(String, String, Option<String>, Option<String>, Option<String>)> =
        sqlx::query_as(
            "SELECT id, kind, auth_type, key_path, ssh_profile_id FROM explorer_connections",
        )
        .fetch_all(pool)
        .await
        .map_err(|e| format!("probe explorer: {}", e))?;
    for (id, kind, auth_type, key_path, ssh_profile_id) in rows {
        let needs = match kind.as_str() {
            "sftp" => {
                // Tunnel-via-saved-profile case is handled by the ssh
                // bucket; the explorer row itself is fine.
                if ssh_profile_id.is_some() {
                    false
                } else {
                    match auth_type.as_deref() {
                        Some("password") => !has_explorer_secret(&store, &id, "password").await,
                        Some("key") => match key_path {
                            Some(p) if !p.trim().is_empty() => {
                                !std::path::Path::new(&p).exists()
                            }
                            _ => true,
                        },
                        _ => false,
                    }
                }
            }
            "ftp" => !has_explorer_secret(&store, &id, "password").await,
            "s3" => {
                let has_access = has_explorer_secret(&store, &id, "access_key").await;
                let has_secret = has_explorer_secret(&store, &id, "secret_key").await;
                !(has_access && has_secret)
            }
            "azure_blob" => {
                // Any one of the three is enough to authenticate.
                let any = has_explorer_secret(&store, &id, "shared_key").await
                    || has_explorer_secret(&store, &id, "sas_token").await
                    || has_explorer_secret(&store, &id, "connection_string").await;
                !any
            }
            _ => false,
        };
        if needs {
            out.explorer.push(id);
        }
    }

    Ok(out)
}

async fn has_secret<S: CredentialStore + ?Sized>(store: &S, key: &str) -> bool {
    store
        .get(key)
        .await
        .ok()
        .flatten()
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

async fn has_explorer_secret<S: CredentialStore + ?Sized>(
    store: &S,
    id: &str,
    secret_name: &str,
) -> bool {
    has_secret(store, &format!("explorer:{}:{}", id, secret_name)).await
}
