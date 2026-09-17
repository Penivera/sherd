//! SQLite-backed storage. Schema only for now — `contacts`/`conversations`/
//! `messages`/`peer_links` mirror `models.rs` and exist so the shape is
//! settled before the mesh-relay milestone starts writing to them.

use std::path::Path;

use rusqlite::Connection;

pub struct Storage {
    conn: Connection,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS contacts (
    id INTEGER PRIMARY KEY,
    display_name TEXT NOT NULL,
    device_id TEXT NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id)
);
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY,
    conversation_id INTEGER NOT NULL REFERENCES conversations(id),
    body TEXT NOT NULL,
    status TEXT NOT NULL,
    created_at_unix INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS peer_links (
    id INTEGER PRIMARY KEY,
    contact_id INTEGER NOT NULL REFERENCES contacts(id),
    ssid TEXT NOT NULL,
    last_seen_unix INTEGER NOT NULL
);
";

impl Storage {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::from_connection(conn)
    }

    fn from_connection(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_and_migrates_in_memory() {
        let storage = Storage::open_in_memory().expect("open");
        let count: i64 = storage
            .connection()
            .query_row("SELECT COUNT(*) FROM contacts", [], |row| row.get(0))
            .expect("query");
        assert_eq!(count, 0);
    }
}
