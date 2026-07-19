use sea_orm::entity::prelude::*;

/// SeaORM entity for the `cli_auth_codes` table.
///
/// A row is a single-use, short-lived authorization code backing the CLI's
/// browser (loopback) sign-in (see `crate::handlers::cli_auth`). `code_hash` is
/// the SHA-256 hex of the one-time code handed to the CLI via the loopback
/// redirect; the plaintext is never stored. `code_challenge` is the SHA-256 hex
/// of a PKCE verifier the CLI keeps private, so intercepting the code as it rides
/// the redirect URL is useless without the verifier. `session_version` records
/// the account generation the code was minted under, so a password reset in the
/// (short) window invalidates it; `consumed_at` marks it spent (claimed once).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "cli_auth_codes")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    #[sea_orm(unique)]
    pub code_hash: String,
    pub code_challenge: String,
    pub session_version: i64,
    pub client_name: Option<String>,
    pub expires_at: DateTimeUtc,
    pub consumed_at: Option<DateTimeUtc>,
    pub created_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
