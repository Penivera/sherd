//! SQLite-backed storage using SeaORM. Schema only for now — `contacts`/`conversations`/
//! `messages`/`peer_links` mirror `models.rs` and exist so the shape is
//! settled before the mesh-relay milestone starts writing to them.

use std::path::Path;
use sea_orm::{Database, DatabaseConnection, DbErr, Schema, ConnectionTrait};

pub struct Storage {
    db: DatabaseConnection,
}

impl Storage {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, DbErr> {
        let url = format!("sqlite://{}?mode=rwc", path.as_ref().display());
        let db = Database::connect(url).await?;
        Self::from_connection(db).await
    }

    pub async fn open_in_memory() -> Result<Self, DbErr> {
        let db = Database::connect("sqlite::memory:").await?;
        Self::from_connection(db).await
    }

    async fn from_connection(db: DatabaseConnection) -> Result<Self, DbErr> {
        let builder = db.get_database_backend();
        let schema = Schema::new(builder);

        let stmts = vec![
            builder.build(schema.create_table_from_entity(crate::entity::contact::Entity).if_not_exists()),
            builder.build(schema.create_table_from_entity(crate::entity::conversation::Entity).if_not_exists()),
            builder.build(schema.create_table_from_entity(crate::entity::message::Entity).if_not_exists()),
            builder.build(schema.create_table_from_entity(crate::entity::peer_link::Entity).if_not_exists()),
        ];

        for stmt in stmts {
            db.execute(stmt).await?;
        }

        Ok(Self { db })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.db
    }
}
