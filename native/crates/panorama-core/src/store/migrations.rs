use rusqlite::{Connection, TransactionBehavior};

use super::StoreError;

const MIGRATIONS: &[&str] = &["CREATE TABLE kv (
         key TEXT PRIMARY KEY,
         value BLOB NOT NULL,
         updated_at INTEGER NOT NULL
     ) WITHOUT ROWID;
     CREATE INDEX kv_cache_updated_at ON kv(updated_at, key)
         WHERE key GLOB 'catalog:*' OR key GLOB 'meta:*';"];

/// Current SQLite schema version; separate from the core's saved schema bucket.
pub const SCHEMA_VERSION: i64 = MIGRATIONS.len() as i64;

pub(super) fn version(connection: &Connection) -> Result<i64, StoreError> {
    let found = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if found > SCHEMA_VERSION {
        return Err(StoreError::NewerSchema {
            found,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(found)
}

pub(super) fn migrate(connection: &mut Connection) -> Result<(), StoreError> {
    if version(connection)? == SCHEMA_VERSION {
        return Ok(());
    }
    let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let found = version(&transaction)?;
    for (index, sql) in MIGRATIONS.iter().enumerate() {
        let target = index as i64 + 1;
        if found < target {
            transaction.execute_batch(sql)?;
            transaction.pragma_update(None, "user_version", target)?;
        }
    }
    transaction.commit()?;
    Ok(())
}
