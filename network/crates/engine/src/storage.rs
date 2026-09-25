//! SQLite-backed storage using SeaORM for the `contacts`/`conversations`/
//! `messages`/`peer_links` tables mirrored in `models.rs`. Written to by
//! `service.rs`'s mailbox as messages are sent and received.

use std::path::Path;

use sea_orm::{
    ActiveModelTrait, ColumnTrait, Database, DatabaseConnection, DbErr,
    EntityTrait, QueryFilter, QueryOrder, Set,
};

use crate::entity::{contact, conversation, message};

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
        db.get_schema_registry("engine::entity").sync(&db).await?;

        Ok(Self { db })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        &self.db
    }

    /// Look up a contact by `device_id`, creating one if this is the first
    /// time this device has been seen. Refreshes the stored display name if
    /// it's changed since last time (devices can rename themselves).
    pub async fn find_or_create_contact(
        &self,
        device_id: &str,
        display_name: &str,
    ) -> Result<i32, DbErr> {
        if let Some(existing) =
            contact::Entity::find().filter(contact::Column::DeviceId.eq(device_id)).one(&self.db).await?
        {
            if existing.display_name != display_name {
                let mut active: contact::ActiveModel = existing.clone().into();
                active.display_name = Set(display_name.to_string());
                active.update(&self.db).await?;
            }
            return Ok(existing.id);
        }

        let active = contact::ActiveModel {
            display_name: Set(display_name.to_string()),
            device_id: Set(device_id.to_string()),
            ..Default::default()
        };
        Ok(active.insert(&self.db).await?.id)
    }

    /// The one conversation this device has with a contact (1:1 only for
    /// now -- no group conversations), creating it if this is the first
    /// message exchanged with them.
    pub async fn find_or_create_conversation(&self, contact_id: i32) -> Result<i32, DbErr> {
        if let Some(existing) = conversation::Entity::find()
            .filter(conversation::Column::ContactId.eq(contact_id))
            .one(&self.db)
            .await?
        {
            return Ok(existing.id);
        }

        let active = conversation::ActiveModel { contact_id: Set(contact_id), ..Default::default() };
        Ok(active.insert(&self.db).await?.id)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_message(
        &self,
        conversation_id: i32,
        direction: &str,
        body: Option<&str>,
        attachment_name: Option<&str>,
        attachment_path: Option<&str>,
        status: &str,
        created_at_unix: i64,
    ) -> Result<(), DbErr> {
        let active = message::ActiveModel {
            conversation_id: Set(conversation_id),
            direction: Set(direction.to_string()),
            body: Set(body.map(|s| s.to_string())),
            attachment_name: Set(attachment_name.map(|s| s.to_string())),
            attachment_path: Set(attachment_path.map(|s| s.to_string())),
            status: Set(status.to_string()),
            created_at_unix: Set(created_at_unix),
            ..Default::default()
        };
        active.insert(&self.db).await?;
        Ok(())
    }

    /// Resolve an exact `device_id` or an unambiguous prefix of one (see
    /// `mailbox::PeerRegistry::resolve` -- same idea, for contacts we have
    /// *history* with rather than ones currently reachable) to a contact.
    async fn contact_by_id_or_prefix(&self, id_or_prefix: &str) -> Result<Option<contact::Model>, DbErr> {
        if let Some(exact) =
            contact::Entity::find().filter(contact::Column::DeviceId.eq(id_or_prefix)).one(&self.db).await?
        {
            return Ok(Some(exact));
        }
        let mut matches = contact::Entity::find()
            .filter(contact::Column::DeviceId.like(format!("{id_or_prefix}%")))
            .all(&self.db)
            .await?;
        Ok(if matches.len() == 1 { matches.pop() } else { None })
    }

    /// Full message history with a contact, oldest first. Empty (not an
    /// error) if this device has never exchanged anything with them.
    pub async fn messages_for_device(&self, device_id: &str) -> Result<Vec<message::Model>, DbErr> {
        let Some(contact) = self.contact_by_id_or_prefix(device_id).await? else {
            return Ok(Vec::new());
        };
        let Some(conv) = conversation::Entity::find()
            .filter(conversation::Column::ContactId.eq(contact.id))
            .one(&self.db)
            .await?
        else {
            return Ok(Vec::new());
        };
        message::Entity::find()
            .filter(message::Column::ConversationId.eq(conv.id))
            .order_by_asc(message::Column::CreatedAtUnix)
            .all(&self.db)
            .await
    }
}
