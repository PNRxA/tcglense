use sea_orm::entity::prelude::*;

/// SeaORM entity for the `collection_product_items` table.
///
/// One row per `(user, game, product)` records how many copies of a sealed product a
/// signed-in user owns. A product with both counts at zero has no row. Product ids are
/// the catalog's internal integer ids; the HTTP surface resolves the provider's external
/// id before reading or writing this table. Like its wish-list twin, `product_id` is
/// deliberately FK-less: the daily TCGCSV sweep's one delete (a product the feed
/// reclassifies as a single card) orphans the row, and the reads — including the
/// paginated list's `total` — skip it.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "collection_product_items")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub user_id: i32,
    pub game: String,
    pub product_id: i32,
    pub quantity: i32,
    pub foil_quantity: i32,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::product::Entity",
        from = "Column::ProductId",
        to = "super::product::Column::Id"
    )]
    Product,
}

impl Related<super::product::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Product.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
