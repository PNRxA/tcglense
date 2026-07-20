use sea_orm::entity::prelude::*;

/// SeaORM entity for the `release_notifications` table: the per-user ledger of release
/// heads-ups already delivered, so [`crate::release_alerts`] is edge-triggered and never
/// re-notifies a user about the same upcoming Secret Lair drop or set.
///
/// One row per `(user_id, kind, ref_key)` (a unique index): `kind` is `"sld_drop"` (a Secret
/// Lair drop, keyed by its slug) or `"set"` (a regular set, keyed by its code), and `ref_key`
/// is that stable per-release key. A row is inserted only after a channel actually accepted
/// the notification, so an undeliverable heads-up (no channel configured yet) is retried on the
/// next pass rather than swallowed — the same "latch only on delivery" contract price alerts use.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "release_notifications")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`). Deleting the user cascades the row away.
    pub user_id: i32,
    /// `"sld_drop"` (Secret Lair drop) or `"set"` (regular set) — which kind of release.
    pub kind: String,
    /// The stable per-release key within the kind: a drop slug, or a set code.
    pub ref_key: String,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// The release date we notified about (ISO `YYYY-MM-DD`), for reference / debugging.
    pub release_date: String,
    pub sent_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
