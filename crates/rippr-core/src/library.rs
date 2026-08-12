use std::{path::Path, sync::Mutex};

use rusqlite::{Connection, params};

use crate::{LibraryEntry, RipError, TrimRange};

pub(crate) struct LibraryStore {
    connection: Mutex<Connection>,
}

impl LibraryStore {
    pub(crate) fn open(path: &Path) -> Result<Self, RipError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "busy_timeout", 5_000)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS library_entries (
                id TEXT PRIMARY KEY,
                source_url TEXT NOT NULL,
                title TEXT NOT NULL,
                creator TEXT,
                source_duration_seconds REAL NOT NULL,
                trim_start_seconds REAL NOT NULL,
                trim_end_seconds REAL NOT NULL,
                rendered_sample_rate INTEGER NOT NULL,
                frame_count INTEGER NOT NULL,
                media_path TEXT NOT NULL,
                created_at TEXT NOT NULL
            );",
        )?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub(crate) fn insert(&self, entry: &LibraryEntry) -> Result<(), RipError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RipError::LibraryUnavailable)?;
        connection.execute(
            "INSERT OR REPLACE INTO library_entries (
                id, source_url, title, creator, source_duration_seconds,
                trim_start_seconds, trim_end_seconds, rendered_sample_rate,
                frame_count, media_path, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                entry.id,
                entry.source_url,
                entry.title,
                entry.creator,
                entry.source_duration_seconds,
                entry.trim.start_seconds,
                entry.trim.end_seconds,
                entry.rendered_sample_rate,
                entry.frame_count,
                entry.media_path.to_string_lossy(),
                entry.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub(crate) fn get(&self, id: &str) -> Result<Option<LibraryEntry>, RipError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RipError::LibraryUnavailable)?;
        let mut statement = connection.prepare(
            "SELECT id, source_url, title, creator, source_duration_seconds,
                    trim_start_seconds, trim_end_seconds, rendered_sample_rate,
                    frame_count, media_path, created_at
             FROM library_entries WHERE id = ?1",
        )?;
        let mut rows = statement.query([id])?;
        let Some(row) = rows.next()? else {
            return Ok(None);
        };
        Ok(Some(entry_from_row(row)?))
    }

    pub(crate) fn list(&self) -> Result<Vec<LibraryEntry>, RipError> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| RipError::LibraryUnavailable)?;
        let mut statement = connection.prepare(
            "SELECT id, source_url, title, creator, source_duration_seconds,
                    trim_start_seconds, trim_end_seconds, rendered_sample_rate,
                    frame_count, media_path, created_at
             FROM library_entries ORDER BY created_at DESC",
        )?;
        let entries = statement
            .query_map([], entry_from_row)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(entries)
    }
}

fn entry_from_row(row: &rusqlite::Row<'_>) -> Result<LibraryEntry, rusqlite::Error> {
    let created_at: String = row.get(10)?;
    Ok(LibraryEntry {
        id: row.get(0)?,
        source_url: row.get(1)?,
        title: row.get(2)?,
        creator: row.get(3)?,
        source_duration_seconds: row.get(4)?,
        trim: TrimRange {
            start_seconds: row.get(5)?,
            end_seconds: row.get(6)?,
        },
        rendered_sample_rate: row.get(7)?,
        frame_count: row.get(8)?,
        media_path: row.get::<_, String>(9)?.into(),
        created_at: chrono::DateTime::parse_from_rfc3339(&created_at)
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    10,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?
            .with_timezone(&chrono::Utc),
    })
}
