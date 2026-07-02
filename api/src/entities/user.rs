use sea_orm::entity::prelude::*;

/// SeaORM entity for the `users` table.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    #[sea_orm(unique)]
    pub email: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
    /// When the account's email address was verified (via an emailed link, or a
    /// completed password reset — both prove mailbox ownership). `None` means
    /// unverified, and login is refused until it is set. Accounts predating the
    /// column were backfilled as verified.
    pub email_verified_at: Option<DateTimeUtc>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
