//! Persistence layer for the ZeroAny Workbench SQLite database.
//!
//! - `pool`          — connection pool initialization.
//! - `migrator`      — schema migration runner; embeds `migrations/*.sql`
//!                     at compile time and applies any unseen versions.
//! - `bootstrap`     — seeds `_sqlx_migrations` for databases that pre-date
//!                     this migrator so we don't re-run V1–Vn against
//!                     already-migrated schemas.
//! - `models`        — sqlx::FromRow structs shared across modes.

pub mod bootstrap;
pub mod migrator;
pub mod models;
pub mod pool;
