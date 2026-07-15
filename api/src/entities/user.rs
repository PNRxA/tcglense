use sea_orm::entity::prelude::*;

/// SeaORM entity for the `users` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub email: String,
    /// `None` marks a **pending registration** (email-first flow, issue #176):
    /// the address was submitted and a completion link emailed, but no password
    /// has been chosen yet. Such an account cannot sign in; completing the
    /// registration (or a password reset) sets the hash.
    pub password_hash: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    /// When the account's email address was verified (via an emailed link, or a
    /// completed password reset — both prove mailbox ownership). `None` means
    /// unverified, and login is refused until it is set. Accounts predating the
    /// column were backfilled as verified.
    pub email_verified_at: Option<DateTimeUtc>,
    /// Opt-in public handle (issue #362). `None` until the user first makes a
    /// collection public. Stored case-preserving; case-insensitive uniqueness is
    /// enforced on the `(username, discriminator)` pair (SQLite `COLLATE NOCASE`,
    /// Postgres `lower(username)` index). Always set/cleared together with
    /// `discriminator`; the `rustrict` blocklist + charset/length rules are applied
    /// in the handler, not the DB.
    pub username: Option<String>,
    /// Discord-style tag (allocated `1..=9999`) that lets several users share a
    /// `username` — the pair is unique, so one "alice" can be #0001 and another
    /// "alice" #0002. Displayed zero-padded as `#0001`; `None` whenever `username`
    /// is `None`.
    pub discriminator: Option<i32>,
    /// ISO 4217 code used to convert canonical USD prices for display (issue #412).
    /// Writes are restricted to `currency::SUPPORTED_CURRENCIES`; existing accounts and
    /// new registrations default to USD in the migration/database.
    pub currency: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
