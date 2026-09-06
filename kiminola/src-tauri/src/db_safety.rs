//! Database startup, migration snapshots and startup-only recovery.
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use sqlx::migrate::Migrator;
use sqlx::sqlite::{SqliteConnectOptions, SqliteLockingMode, SqlitePoolOptions};
use sqlx::SqlitePool;
use tokio::sync::Mutex;

static MIGRATIONS: Migrator = sqlx::migrate!("./migrations");

#[derive(Default)]
struct Runtime {
    pool: Option<SqlitePool>,
    error: Option<String>,
    suspended: bool,
    // Held for the process lifetime, including startup errors and recovery.
    owner: Option<File>,
}

pub struct Database {
    path: Result<PathBuf, String>,
    runtime: Mutex<Runtime>,
}

#[derive(serde::Serialize)]
pub struct DatabaseStatus {
    pub ready: bool,
    pub error: Option<String>,
    pub database_path: String,
    pub backup_directory: String,
    pub backups: Vec<String>,
    pub backup_error: Option<String>,
}

fn io_error(error: impl std::fmt::Display) -> String {
    error.to_string()
}

fn exists(path: &Path) -> Result<bool, String> {
    path.try_exists().map_err(io_error)
}

fn backup_dir(path: &Path) -> PathBuf {
    path.parent().expect("database parent").join("backups")
}

fn restore_marker(path: &Path) -> PathBuf {
    path.with_extension("restore-pending")
}

fn unique_name(prefix: &str) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{prefix}-{}-{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn acquire_owner(path: &Path) -> Result<File, String> {
    std::fs::create_dir_all(path.parent().ok_or("database has no parent")?).map_err(io_error)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    options.open(path.with_extension("owner-lock"))
        .map_err(|e| format!("Cannot exclusively open the data folder. Close other Kimi Nola instances and retry: {e}"))
}

async fn connect(path: &Path, read_only: bool) -> Result<SqlitePool, String> {
    let mut options = SqliteConnectOptions::new()
        .filename(path)
        .read_only(read_only)
        .create_if_missing(!read_only);
    if !read_only {
        options = options.locking_mode(SqliteLockingMode::Exclusive);
    }
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .map_err(io_error)
}

async fn verify(pool: &SqlitePool) -> Result<(), String> {
    let integrity: Vec<String> = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_all(pool)
        .await
        .map_err(io_error)?;
    if integrity != ["ok"] {
        return Err(format!(
            "Database integrity check failed: {}",
            integrity.join("; ")
        ));
    }
    if !sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(pool)
        .await
        .map_err(io_error)?
        .is_empty()
    {
        return Err("Database contains broken references; recovery is required.".into());
    }
    Ok(())
}

async fn check_history(pool: &SqlitePool, migrations: &Migrator) -> Result<bool, String> {
    let has_history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='_sqlx_migrations'")
            .fetch_one(pool)
            .await
            .map_err(io_error)?;
    if has_history == 0 {
        let tables: i64 =
            sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")
                .fetch_one(pool)
                .await
                .map_err(io_error)?;
        if tables != 0 {
            return Err(
                "Existing database has no migration history. It has not been replaced.".into(),
            );
        }
        return Ok(!migrations.migrations.is_empty());
    }
    let applied: Vec<(i64, Vec<u8>, bool)> =
        sqlx::query_as("SELECT version, checksum, success FROM _sqlx_migrations")
            .fetch_all(pool)
            .await
            .map_err(io_error)?;
    for (version, checksum, success) in &applied {
        let migration = migrations.iter().find(|m| m.version == *version)
            .ok_or_else(|| format!("This database requires a newer Kimi Nola version (migration {version}). Install the newer app; do not downgrade."))?;
        if !success || migration.checksum.as_ref() != checksum.as_slice() {
            return Err(format!("Migration {version} history does not match this app. The database has been preserved."));
        }
    }
    Ok(migrations.iter().any(|m| {
        !m.migration_type.is_down_migration() && !applied.iter().any(|a| a.0 == m.version)
    }))
}

async fn snapshot(pool: &SqlitePool, destination: &Path) -> Result<(), String> {
    // VACUUM INTO includes committed WAL data and never copies a live DB file blindly.
    sqlx::query("VACUUM main INTO ?")
        .bind(destination.to_string_lossy().as_ref())
        .execute(pool)
        .await
        .map_err(|e| format!("Cannot create database backup: {e}"))?;
    let copy = connect(destination, true).await?;
    let result = verify(&copy).await;
    copy.close().await;
    result?;
    OpenOptions::new()
        .write(true)
        .open(destination)
        .and_then(|file| file.sync_all())
        .map_err(io_error)
}

async fn migrate(
    pool: &SqlitePool,
    path: &Path,
    migrations: &Migrator,
    keep_backup: bool,
) -> Result<(), String> {
    // In EXCLUSIVE locking mode this ownership lasts until the connection closes.
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(pool)
        .await
        .map_err(io_error)?;
    sqlx::query("COMMIT")
        .execute(pool)
        .await
        .map_err(io_error)?;
    verify(pool).await?;
    let migrations = crate::migrations::compatible(pool, migrations).await?;
    if !check_history(pool, &migrations).await? {
        return Ok(());
    }
    let has_history: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_master WHERE name='_sqlx_migrations'")
            .fetch_one(pool)
            .await
            .map_err(io_error)?;
    if keep_backup && has_history != 0 {
        let directory = backup_dir(path);
        std::fs::create_dir_all(&directory).map_err(io_error)?;
        let name = unique_name("before-migration");
        let partial = directory.join(format!("{name}.partial"));
        snapshot(pool, &partial).await?;
        std::fs::rename(&partial, directory.join(format!("{name}.db"))).map_err(io_error)?;
    }
    migrations
        .run(pool)
        .await
        .map_err(|e| format!("Database migration failed: {e}"))?;
    verify(pool).await
}

pub(crate) async fn open_migrated(
    path: &Path,
    migrations: &Migrator,
    keep_backup: bool,
) -> Result<SqlitePool, String> {
    if exists(&restore_marker(path))? {
        return Err("A previous database restore was interrupted. Choose a verified backup to finish recovery; no empty database was created.".into());
    }
    if exists(path)? && std::fs::metadata(path).map_err(io_error)?.len() == 0 {
        return Err(
            "The existing database file is empty. It has been preserved for recovery.".into(),
        );
    }
    std::fs::create_dir_all(path.parent().ok_or("database has no parent")?).map_err(io_error)?;
    let pool = connect(path, false).await?;
    if let Err(error) = migrate(&pool, path, migrations, keep_backup).await {
        pool.close().await;
        return Err(error);
    }
    Ok(pool)
}

pub(crate) async fn init_pool(path: &Path) -> Result<SqlitePool, String> {
    open_migrated(path, &MIGRATIONS, true).await
}

impl Database {
    pub fn new(path: Result<PathBuf, String>) -> Self {
        Self {
            path,
            runtime: Mutex::new(Runtime::default()),
        }
    }

    async fn initialize(&self, runtime: &mut Runtime) -> Result<SqlitePool, String> {
        let path = self.path.as_ref().map_err(Clone::clone)?;
        if runtime.owner.is_none() {
            runtime.owner = Some(acquire_owner(path)?);
        }
        init_pool(path).await
    }

    pub async fn pool(&self) -> Result<SqlitePool, String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.suspended {
            return Err("The app is preparing to update. Try again after it restarts.".into());
        }
        if let Some(pool) = &runtime.pool {
            return Ok(pool.clone());
        }
        if let Some(error) = &runtime.error {
            return Err(error.clone());
        }
        match self.initialize(&mut runtime).await {
            Ok(pool) => {
                runtime.pool = Some(pool.clone());
                Ok(pool)
            }
            Err(error) => {
                runtime.error = Some(error.clone());
                Err(error)
            }
        }
    }

    pub async fn status(&self) -> DatabaseStatus {
        let result = self.pool().await;
        let path = self.path.as_ref().ok();
        let (mut backups, backup_error) = match path.map(|p| list_backups(p)) {
            Some(Ok(backups)) => (backups, None),
            Some(Err(error)) => (
                Vec::new(),
                Some(format!("Could not list migration backups: {error}")),
            ),
            None => (Vec::new(), None),
        };
        backups.sort_by(|a, b| b.cmp(a));
        DatabaseStatus {
            ready: result.is_ok(),
            error: result.err(),
            database_path: path.map(|p| p.display().to_string()).unwrap_or_default(),
            backup_directory: path
                .map(|p| backup_dir(p).display().to_string())
                .unwrap_or_default(),
            backups,
            backup_error,
        }
    }

    pub async fn retry(&self) -> Result<(), String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.pool.is_some() || runtime.suspended {
            return Err(
                "Recovery retry is available only when the database is unavailable.".into(),
            );
        }
        runtime.error = None;
        Ok(())
    }

    pub async fn suspend(&self) -> Result<(), String> {
        self.pool().await?;
        let mut runtime = self.runtime.lock().await;
        runtime.suspended = true;
        if let Some(pool) = runtime.pool.take() {
            pool.close().await;
        }
        Ok(())
    }

    pub async fn resume(&self) {
        self.runtime.lock().await.suspended = false;
    }

    pub async fn prepare_restart(&self) -> Result<(), String> {
        self.suspend().await?;
        // Tauri can spawn the replacement before this process has exited.
        // Keep this process sealed, but let the replacement claim the data.
        self.runtime.lock().await.owner = None;
        Ok(())
    }

    pub async fn restore(&self, name: &str) -> Result<(), String> {
        let mut runtime = self.runtime.lock().await;
        if runtime.pool.is_some() || runtime.suspended || runtime.error.is_none() {
            return Err("Restore is available only after database startup has failed.".into());
        }
        let path = self.path.as_ref().map_err(Clone::clone)?;
        if runtime.owner.is_none() {
            runtime.owner = Some(acquire_owner(path)?);
        }
        restore_backup(path, name).await?;
        runtime.error = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::{Migration, MigrationType};
    use std::borrow::Cow;
    use std::sync::Arc;

    struct Fixture {
        root: PathBuf,
        path: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(unique_name("kiminola-db-safety-test"));
            std::fs::create_dir(&root).unwrap();
            Self {
                path: root.join("kiminola.db"),
                root,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            assert!(self.root.starts_with(std::env::temp_dir()));
            assert!(self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("kiminola-db-safety-test-"));
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
    fn prefix(count: usize) -> Migrator {
        Migrator {
            migrations: Cow::Owned(MIGRATIONS.migrations[..count].to_vec()),
            ..Migrator::DEFAULT
        }
    }
    async fn seed(path: &Path, count: usize) -> SqlitePool {
        let pool = open_migrated(path, &prefix(count), true).await.unwrap();
        sqlx::query("INSERT INTO meetings(id,title,space_id,created_at,duration_seconds) VALUES(42,'Migration fixture',1,'2026-01-01',99)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO notes(meeting_id,raw_markdown,updated_at) VALUES(42,'preserve notes','2026-01-01')").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO transcript_segments(meeting_id,channel,text,start_ms,end_ms) VALUES(42,'you','preserve transcript',1,50)").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO settings(key,value) VALUES('fixture','before upgrade')")
            .execute(&pool)
            .await
            .unwrap();
        if count >= 2 {
            sqlx::query("INSERT INTO templates(name,prompt,is_builtin) VALUES('Custom','preserve prompt',0)").execute(&pool).await.unwrap();
            sqlx::query(
                "UPDATE notes SET enhanced_markdown='preserve enhanced' WHERE meeting_id=42",
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        if count >= 5 {
            sqlx::query("INSERT INTO note_drafts(id,title,created_at,updated_at,raw_markdown) VALUES(43,'draft','2026-01-01','2026-01-01','preserve draft')").execute(&pool).await.unwrap();
        }
        if count >= 6 {
            sqlx::query("UPDATE note_drafts SET recovery_transcript_json='[{\"channel\":\"you\",\"text\":\"recovery\"}]',recovery_duration_seconds=15").execute(&pool).await.unwrap();
        }
        if count >= 8 {
            sqlx::query(
                "UPDATE note_drafts SET recovery_location_json='{\"kind\":\"space\",\"id\":1}'",
            )
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }
    async fn marker(pool: &SqlitePool) -> String {
        sqlx::query_scalar("SELECT value FROM settings WHERE key='fixture'")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn every_populated_schema_upgrades_and_keeps_a_readable_original() {
        for version in 1..=MIGRATIONS.migrations.len() {
            let fixture = Fixture::new();
            seed(&fixture.path, version).await.close().await;
            let upgraded = init_pool(&fixture.path).await.unwrap();
            assert_eq!(marker(&upgraded).await, "before upgrade");
            let content: (String, String, i64, i64) = sqlx::query_as("SELECT n.raw_markdown,t.text,t.start_ms,t.end_ms FROM notes n JOIN transcript_segments t ON t.meeting_id=n.meeting_id WHERE n.meeting_id=42").fetch_one(&upgraded).await.unwrap();
            assert_eq!(
                content,
                ("preserve notes".into(), "preserve transcript".into(), 1, 50)
            );
            if version >= 2 {
                let prompt: String =
                    sqlx::query_scalar("SELECT prompt FROM templates WHERE is_builtin=0")
                        .fetch_one(&upgraded)
                        .await
                        .unwrap();
                assert_eq!(prompt, "preserve prompt");
                let enhanced: String =
                    sqlx::query_scalar("SELECT enhanced_markdown FROM notes WHERE meeting_id=42")
                        .fetch_one(&upgraded)
                        .await
                        .unwrap();
                assert_eq!(enhanced, "preserve enhanced");
            }
            if version >= 5 {
                let text: String =
                    sqlx::query_scalar("SELECT raw_markdown FROM note_drafts WHERE id=43")
                        .fetch_one(&upgraded)
                        .await
                        .unwrap();
                assert_eq!(text, "preserve draft");
            }
            if version >= 6 {
                let data: (String, i64) = sqlx::query_as("SELECT recovery_transcript_json,recovery_duration_seconds FROM note_drafts WHERE id=43").fetch_one(&upgraded).await.unwrap();
                assert!(data.0.contains("recovery"));
                assert_eq!(data.1, 15);
            }
            if version >= 8 {
                let location: String = sqlx::query_scalar(
                    "SELECT recovery_location_json FROM note_drafts WHERE id=43",
                )
                .fetch_one(&upgraded)
                .await
                .unwrap();
                assert_eq!(location, "{\"kind\":\"space\",\"id\":1}");
            }
            verify(&upgraded).await.unwrap();
            upgraded.close().await;
            let backups = list_backups(&fixture.path).unwrap();
            assert_eq!(
                backups.len(),
                usize::from(version < MIGRATIONS.migrations.len())
            );
            if let Some(name) = backups.first() {
                let original = connect(&backup_dir(&fixture.path).join(name), true)
                    .await
                    .unwrap();
                verify(&original).await.unwrap();
                assert_eq!(marker(&original).await, "before upgrade");
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
                    .fetch_one(&original)
                    .await
                    .unwrap();
                assert_eq!(count, version as i64);
                original.close().await;
            }
            init_pool(&fixture.path).await.unwrap().close().await;
            assert_eq!(
                list_backups(&fixture.path).unwrap(),
                backups,
                "unchanged schema must not make another backup"
            );
        }
    }

    #[tokio::test]
    async fn backup_failure_prevents_schema_changes() {
        let fixture = Fixture::new();
        seed(&fixture.path, 1).await.close().await;
        std::fs::write(backup_dir(&fixture.path), b"not a directory").unwrap();
        assert!(init_pool(&fixture.path).await.is_err());
        let original = connect(&fixture.path, true).await.unwrap();
        assert_eq!(marker(&original).await, "before upgrade");
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(&original)
            .await
            .unwrap();
        assert_eq!(count, 1);
        original.close().await;
    }

    #[tokio::test]
    async fn failed_sql_rolls_back_and_retains_backup() {
        let fixture = Fixture::new();
        seed(&fixture.path, 9).await.close().await;
        let mut migrations = prefix(9);
        migrations.migrations.to_mut().push(Migration::new(10, "failing fixture".into(), MigrationType::Simple,
            "ALTER TABLE meetings ADD COLUMN failure TEXT; UPDATE settings SET value='wrong' WHERE key='fixture'; INSERT INTO absent_table VALUES(1);".into(), false));
        assert!(open_migrated(&fixture.path, &migrations, true)
            .await
            .unwrap_err()
            .contains("migration failed"));
        let original = connect(&fixture.path, true).await.unwrap();
        assert_eq!(marker(&original).await, "before upgrade");
        assert!(sqlx::query("SELECT failure FROM meetings")
            .fetch_all(&original)
            .await
            .is_err());
        original.close().await;
        assert_eq!(list_backups(&fixture.path).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn snapshot_includes_committed_wal_data() {
        let fixture = Fixture::new();
        let pool = seed(&fixture.path, 1).await;
        sqlx::query("PRAGMA journal_mode=WAL")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint=0")
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("UPDATE settings SET value='committed WAL data' WHERE key='fixture'")
            .execute(&pool)
            .await
            .unwrap();
        migrate(&pool, &fixture.path, &MIGRATIONS, true)
            .await
            .unwrap();
        let names = list_backups(&fixture.path).unwrap();
        let snapshot = connect(&backup_dir(&fixture.path).join(&names[0]), true)
            .await
            .unwrap();
        assert_eq!(marker(&snapshot).await, "committed WAL data");
        snapshot.close().await;
        pool.close().await;
    }

    #[tokio::test]
    async fn corrupted_backup_cannot_replace_the_original() {
        let fixture = Fixture::new();
        seed(&fixture.path, 1).await.close().await;
        init_pool(&fixture.path).await.unwrap().close().await;
        let original_bytes = std::fs::read(&fixture.path).unwrap();
        let names = list_backups(&fixture.path).unwrap();
        std::fs::write(backup_dir(&fixture.path).join(&names[0]), b"not a database").unwrap();
        assert!(restore_backup(&fixture.path, &names[0]).await.is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), original_bytes);
        assert!(!restore_marker(&fixture.path).exists());
        std::fs::write(backup_dir(&fixture.path).join(&names[0]), []).unwrap();
        assert!(restore_backup(&fixture.path, &names[0]).await.is_err());
        assert_eq!(std::fs::read(&fixture.path).unwrap(), original_bytes);
    }

    #[tokio::test]
    async fn history_mismatch_and_downgrade_are_blocked() {
        let fixture = Fixture::new();
        let original = seed(&fixture.path, 9).await;
        original.close().await;
        assert!(open_migrated(&fixture.path, &prefix(8), true)
            .await
            .unwrap_err()
            .contains("newer Kimi Nola"));
        let pool = connect(&fixture.path, false).await.unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=1")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
        assert!(init_pool(&fixture.path)
            .await
            .unwrap_err()
            .contains("history does not match"));
        assert!(list_backups(&fixture.path).unwrap().is_empty());
    }

    #[tokio::test]
    async fn restore_validates_backup_and_archives_original() {
        let fixture = Fixture::new();
        seed(&fixture.path, 1).await.close().await;
        let upgraded = init_pool(&fixture.path).await.unwrap();
        sqlx::query("UPDATE settings SET value='after upgrade' WHERE key='fixture'")
            .execute(&upgraded)
            .await
            .unwrap();
        sqlx::query("UPDATE _sqlx_migrations SET checksum=x'00' WHERE version=1")
            .execute(&upgraded)
            .await
            .unwrap();
        upgraded.close().await;
        let database = Database::new(Ok(fixture.path.clone()));
        assert!(!database.status().await.ready);
        let names = list_backups(&fixture.path).unwrap();
        assert!(database.restore("../kiminola.db").await.is_err());
        database.restore(&names[0]).await.unwrap();
        let restored = database.pool().await.unwrap();
        assert_eq!(marker(&restored).await, "before upgrade");
        restored.close().await;
        let archive = std::fs::read_dir(&fixture.root)
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("before-restore-")
            })
            .unwrap();
        let original = connect(&archive.path().join("kiminola.db"), true)
            .await
            .unwrap();
        assert_eq!(marker(&original).await, "after upgrade");
        original.close().await;
        assert!(!restore_marker(&fixture.path).exists());
    }

    #[tokio::test]
    async fn restore_accepts_legacy_crlf_backup_without_rewriting_history() {
        use sha2::{Digest, Sha384};
        let fixture = Fixture::new();
        let legacy = seed(&fixture.path, 1).await;
        let sql = MIGRATIONS.iter().next().unwrap().sql.replace("\r\n", "\n");
        let checksum = Sha384::digest(sql.replace('\n', "\r\n").as_bytes()).to_vec();
        sqlx::query("UPDATE _sqlx_migrations SET checksum=? WHERE version=1")
            .bind(&checksum).execute(&legacy).await.unwrap();
        legacy.close().await;
        init_pool(&fixture.path).await.unwrap().close().await;
        let names = list_backups(&fixture.path).unwrap();
        restore_backup(&fixture.path, &names[0]).await.unwrap();
        let restored = init_pool(&fixture.path).await.unwrap();
        assert_eq!(marker(&restored).await, "before upgrade");
        let stored: Vec<u8> = sqlx::query_scalar("SELECT checksum FROM _sqlx_migrations WHERE version=1")
            .fetch_one(&restored).await.unwrap();
        assert_eq!(stored, checksum);
        restored.close().await;
    }

    #[tokio::test]
    async fn interrupted_restore_and_empty_file_never_create_an_empty_library() {
        let fixture = Fixture::new();
        std::fs::write(restore_marker(&fixture.path), "interrupted").unwrap();
        assert!(init_pool(&fixture.path)
            .await
            .unwrap_err()
            .contains("interrupted"));
        assert!(!fixture.path.exists());
        std::fs::remove_file(restore_marker(&fixture.path)).unwrap();
        std::fs::write(&fixture.path, []).unwrap();
        assert!(init_pool(&fixture.path)
            .await
            .unwrap_err()
            .contains("empty"));
        assert_eq!(std::fs::metadata(&fixture.path).unwrap().len(), 0);
    }

    #[tokio::test]
    async fn update_barrier_drains_writes_blocks_new_work_and_can_resume() {
        let fixture = Fixture::new();
        let database = Arc::new(Database::new(Ok(fixture.path.clone())));
        let pool = database.pool().await.unwrap();
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("INSERT INTO settings VALUES('fixture','saved before exit')")
            .execute(&mut *transaction)
            .await
            .unwrap();
        let task_database = Arc::clone(&database);
        let task = tokio::spawn(async move { task_database.suspend().await });
        tokio::task::yield_now().await;
        assert!(!task.is_finished());
        transaction.commit().await.unwrap();
        task.await.unwrap().unwrap();
        assert!(database.pool().await.is_err());
        database.resume().await;
        let reopened = database.pool().await.unwrap();
        assert_eq!(marker(&reopened).await, "saved before exit");
        assert!(database.restore("anything").await.is_err());
        assert!(database.retry().await.is_err());
        reopened.close().await;
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn second_owner_cannot_migrate_or_restore_the_same_database() {
        let fixture = Fixture::new();
        let first = Database::new(Ok(fixture.path.clone()));
        let pool = first.pool().await.unwrap();
        let second = Database::new(Ok(fixture.path.clone()));
        assert!(second
            .pool()
            .await
            .unwrap_err()
            .contains("Close other Kimi Nola"));
        pool.close().await;
        first.prepare_restart().await.unwrap();
        assert!(first.pool().await.is_err());
        second.retry().await.unwrap();
        second.pool().await.unwrap().close().await;
    }
}

fn list_backups(path: &Path) -> Result<Vec<String>, String> {
    let directory = backup_dir(path);
    if !exists(&directory)? {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in std::fs::read_dir(directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        if entry.file_type().map_err(io_error)?.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("before-migration-") && name.ends_with(".db") {
                result.push(name);
            }
        }
    }
    Ok(result)
}

async fn restore_backup(path: &Path, name: &str) -> Result<(), String> {
    if !list_backups(path)?
        .iter()
        .any(|candidate| candidate == name)
    {
        return Err("Choose a backup from this database's recovery list.".into());
    }
    let source = backup_dir(path).join(name);
    let stage = path.with_file_name(format!("{}.db", unique_name("restore-stage")));
    let backup = connect(&source, true).await?;
    let prepared = async {
        verify(&backup).await?;
        let applied: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
            .fetch_one(&backup)
            .await
            .map_err(|_| "This file is not a Kimi Nola migration backup.".to_string())?;
        if applied == 0 {
            return Err(
                "This backup has no completed migrations; it cannot replace your data.".into(),
            );
        }
        let migrations = crate::migrations::compatible(&backup, &MIGRATIONS).await?;
        check_history(&backup, &migrations).await?;
        snapshot(&backup, &stage).await
    }
    .await;
    backup.close().await;
    prepared?;
    // Prove the selected backup works with this binary before moving original data.
    let staged = open_migrated(&stage, &MIGRATIONS, false).await?;
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&staged)
        .await
        .map_err(io_error)?;
    staged.close().await;
    OpenOptions::new()
        .write(true)
        .open(&stage)
        .and_then(|f| f.sync_all())
        .map_err(io_error)?;

    let archive = path.parent().unwrap().join(unique_name("before-restore"));
    std::fs::create_dir(&archive).map_err(io_error)?;
    // On Windows deny new readers/writers while permitting our renames. If an
    // older app still has SQLite open, fail before moving any original files.
    let mut originals = Vec::new();
    let mut handles = Vec::new();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut file_name = path.as_os_str().to_os_string();
        file_name.push(suffix);
        let file = PathBuf::from(file_name);
        if exists(&file)? {
            let mut options = OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(4); // FILE_SHARE_DELETE only
            }
            handles.push(options.open(&file).map_err(|e| {
                format!("Close other apps using this database before restoring: {e}")
            })?);
            originals.push(file);
        }
    }
    let marker = restore_marker(path);
    let mut intent = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&marker)
        .map_err(io_error)?;
    writeln!(
        intent,
        "Original database files: {}\nSelected backup: {name}",
        archive.display()
    )
    .map_err(io_error)?;
    intent.sync_all().map_err(io_error)?;
    drop(intent);
    for original in originals {
        std::fs::rename(&original, archive.join(original.file_name().unwrap()))
            .map_err(io_error)?;
    }
    std::fs::rename(&stage, path).map_err(io_error)?;
    drop(handles);
    std::fs::remove_file(marker).map_err(io_error)?;
    Ok(())
}
