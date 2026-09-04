//! Use stable LF checksums for new databases while recognizing the exact CRLF
//! variants shipped by earlier Windows builds. Never rewrite stored checksums.
use sha2::{Digest, Sha384};
use sqlx::migrate::Migration;
use sqlx::SqlitePool;
use std::collections::HashMap;

pub(crate) async fn run(pool: &SqlitePool) -> Result<(), String> {
    let has_history: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_sqlx_migrations')",
    ).fetch_one(pool).await.map_err(|e| e.to_string())?;
    let applied: HashMap<i64, Vec<u8>> = if has_history {
        sqlx::query_as::<_, (i64, Vec<u8>)>("SELECT version, checksum FROM _sqlx_migrations")
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect()
    } else {
        HashMap::new()
    };

    let mut migrator = sqlx::migrate!("./migrations");
    for migration in migrator.migrations.to_mut() {
        let lf = migration.sql.replace("\r\n", "\n");
        let crlf_checksum = Sha384::digest(lf.replace('\n', "\r\n").as_bytes());
        *migration = Migration::new(
            migration.version,
            migration.description.clone(),
            migration.migration_type,
            lf.into(),
            migration.no_tx,
        );
        if let Some(checksum) = applied.get(&migration.version) {
            if checksum.as_slice() == crlf_checksum.as_slice() {
                migration.checksum = checksum.clone().into();
            }
        }
    }
    // SQLx still checks dirty/missing versions and every other checksum mismatch.
    migrator.run(pool).await.map_err(|e| e.to_string())
}
