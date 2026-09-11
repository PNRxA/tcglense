use sea_orm_migration::prelude::*;

/// Adds `sealed_contents.exclusive`: whether a `booster` membership row names a card that
/// **only** this product's booster family can pull — the flag the product page's
/// "Exclusive to Collector Boosters" section splits on.
///
/// Derived once per sync tick by [`crate::catalog::sealed_exclusives`] rather than at read
/// time, for the reason `precon_decks.price_cents` (`m..077`) is a column: the answer is a
/// **cross-product** fact — a card is exclusive to this family when no *other*-family
/// booster in the same set can pull it — so computing it per request meant a scan of every
/// sibling booster's whole pull pool on every `/cards` page turn (measured in production at
/// ~7.2s for one collector booster's 7,968 comparison rows, paid again for page 2 and 3).
/// The set it produces is also a *sort* key (family-exclusive printings lead the shared
/// pool), so it cannot be narrowed to the rows a page happens to show.
///
/// Meaningful **only** on a `membership = 'booster'` row of a product whose own
/// `product_type` is a booster family, and only on rows the *plain* view can see (a card
/// reaching the product through an unlisted component keeps its own certainty split). Every
/// other row stays `false`.
///
/// `DEFAULT false` is load-bearing twice over: it is what lets the several `ActiveModel`
/// literals that spread `..Default::default()` keep inserting, and it is the value the
/// wholesale rebuild writes — the derivation pass fills the column afterwards, exactly as a
/// fresh precon rebuild writes `NULL` prices for `refresh_precon_values` to fold in. So,
/// unlike `m..075`'s `component`, this needs **no** `DERIVATION_VERSION` bump: the pass
/// runs every tick, so the column populates on the first tick after the migration lands
/// without forcing a re-walk of MTGJSON's 600 MB document. Until that tick — up to a full
/// `SYNC_INTERVAL_HOURS`, since `ingest_state::initial_delay` defers it — every booster
/// serves its pool without the exclusive split. That one-interval gap is accepted on
/// purpose, exactly as `m..076` accepted duplicate star tiles until its first tick: the
/// alternative was a full read of every booster's membership rows on every restart,
/// forever, to cover a window that occurs once (see `tasks::spawn_sealed_exclusives`).
///
/// `idx_sealed_contents_unique` is deliberately untouched. That index keys row *identity*
/// (`game, product_id, card_id, membership, foil, component`); `exclusive` is an attribute
/// functionally determined by it, so adding it would change the row grain and break the
/// rebuild's `ON CONFLICT` target.
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SealedContents::Table)
                    .add_column(
                        ColumnDef::new(SealedContents::Exclusive)
                            .boolean()
                            .not_null()
                            .default(false),
                    )
                    .to_owned(),
            )
            .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(SealedContents::Table)
                    .drop_column(SealedContents::Exclusive)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum SealedContents {
    Table,
    Exclusive,
}
