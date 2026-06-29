use sea_orm::entity::prelude::*;

/// SeaORM entity for the `refresh_tokens` table.
///
/// `token_hash` is the SHA-256 hex of the opaque refresh token; the plaintext
/// token is never stored. A `revoked_at` timestamp marks a token as no longer
/// usable (set on rotation, logout, or theft-driven mass revocation).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "refresh_tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub expires_at: DateTimeUtc,
    pub revoked_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
