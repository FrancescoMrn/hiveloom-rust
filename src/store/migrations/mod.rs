use rusqlite::params;

/// Run forward-only, idempotent SQL migrations.
///
/// Each migration is a `(name, sql)` pair. Already-applied migrations (tracked
/// in the `_migrations` table) are skipped. Each new migration runs inside its
/// own transaction.
pub fn run_migrations(conn: &rusqlite::Connection, migrations: &[(&str, &str)]) -> anyhow::Result<()> {
    // Ensure the bookkeeping table exists (idempotent).
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS _migrations (
            name TEXT PRIMARY KEY,
            applied_at TEXT NOT NULL
        );"
    )?;

    for (name, sql) in migrations {
        let already_applied: bool = conn.query_row(
            "SELECT COUNT(*) FROM _migrations WHERE name = ?1",
            params![name],
            |row| row.get::<_, i64>(0),
        )? > 0;

        if already_applied {
            continue;
        }

        // Wrap each migration + bookkeeping row in a single transaction.
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute(
            "INSERT INTO _migrations (name, applied_at) VALUES (?1, ?2)",
            params![name, chrono::Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
    }

    Ok(())
}
