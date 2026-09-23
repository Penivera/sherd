use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Eq)]
#[sea_orm(table_name = "messages")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub conversation_id: i32,
    /// "outgoing" (we sent it) or "incoming" (we received it).
    pub direction: String,
    /// `None` for a file-only message (no accompanying text).
    pub body: Option<String>,
    /// Original filename, set together with `attachment_path` when this
    /// message carried a file.
    pub attachment_name: Option<String>,
    /// Where the file is saved on this device's local disk. For an
    /// "outgoing" message this is the path it was sent from; for an
    /// "incoming" one, where it was saved on arrival.
    pub attachment_path: Option<String>,
    pub status: String,
    pub created_at_unix: i64,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::conversation::Entity",
        from = "Column::ConversationId",
        to = "super::conversation::Column::Id"
    )]
    Conversation,
}

impl Related<super::conversation::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Conversation.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
