use sea_orm::entity::prelude::*;

/// SeaORM entity for the `api_keys` table.
///
/// A user-generated, long-lived credential for the public API. `token_hash` is
/// the SHA-256 hex of the opaque key (`tcgl_<64 hex>`); the plaintext is returned
/// to the caller exactly once at creation and never stored. `key_prefix` keeps the
/// human-visible head of the key (`tcgl_<first 8 hex>`) so the management UI can
/// tell keys apart after the secret is gone.
///
/// `scope` is a string discriminator (`read` / `read_write`) filtered at the
/// authorization seam so a read-only key can't drive a mutation. `revoked_at`
/// marks a key as no longer usable (soft revoke, keeping an audit trail);
/// `expires_at` is an optional lifetime (`NULL` = never expires); `last_used_at`
/// is a best-effort, throttled record of the key's most recent authentication.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "api_keys")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub name: String,
    pub key_prefix: String,
    pub scope: String,
    pub created_at: DateTimeUtc,
    pub last_used_at: Option<DateTimeUtc>,
    pub expires_at: Option<DateTimeUtc>,
    pub revoked_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    /// The key's owner (`user_id` -> `users.id`). Lets the auth path resolve a
    /// presented key and its owning user in one round-trip (`find_also_related`)
    /// instead of a key lookup followed by a separate `users` point read.
    #[sea_orm(
        belongs_to = "super::user::Entity",
        from = "Column::UserId",
        to = "super::user::Column::Id"
    )]
    User,
}

impl Related<super::user::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::User.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
