use sea_orm::entity::prelude::*;

/// SeaORM entity for the `email_tokens` table.
///
/// A row is a single-use, expiring token delivered by email (a verification or
/// password-reset link). `token_hash` is the SHA-256 hex of the emailed token;
/// the plaintext is never stored. `purpose` scopes what the token authorizes so
/// a verification token can never be spent as a password reset (or vice versa),
/// and `consumed_at` marks it spent (each token is claimed exactly once).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "email_tokens")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub purpose: String,
    #[sea_orm(unique)]
    pub token_hash: String,
    pub expires_at: DateTimeUtc,
    pub consumed_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
