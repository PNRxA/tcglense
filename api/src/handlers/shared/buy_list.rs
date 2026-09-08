//! The wish list's **shopping list** (issue #292): the rows a store's bulk-buy page needs,
//! in one authed JSON read — every wanted card with its counts and the TCGplayer product
//! id the ingest already holds (the TCGCSV join key, absent from the shared `Card` DTO on
//! purpose), plus every wanted sealed product by its own TCGplayer id.
//!
//! The list carries **rows, not store URLs**: which stores exist and how each spells a
//! bulk-entry link is the SPA's store registry (`web/src/lib/buyLinks.ts`, the same
//! registry the card page's "Where to buy" buttons come from), so a store added there
//! needs nothing here, and a CLI that wants a different store builds its own link off the
//! same rows. The card rows are the wish-list listing's **own query** — the request
//! resolves through [`super::resolve_holdings_list`] and the twin's `wishlist_query`, like
//! the `.txt` export — narrowed to the handful of columns a line needs, so "buy what's on
//! screen" is the filtered grid and never a second implementation of its filters.
//!
//! Bounded on purpose: a store's mass-entry page is a URL, so the read caps at
//! [`BUY_LIST_MAX_ROWS`] card rows and reports `truncated` rather than streaming an
//! unbounded list the way the export does; the totals on the wire let a client say what
//! was left out.

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, FromQueryResult, QuerySelect,
    QueryTrait, SelectTwo,
};
use serde::Serialize;

use crate::entities::card;
use crate::error::AppError;

use super::product_holdings::ProductHoldingEntry;

/// The most card rows one shopping list carries: a bulk-entry page is a URL, and a
/// TCGplayer mass-entry row by product id is ~10 characters, so 500 rows stays well
/// inside every browser's and server's URL limits.
pub(crate) const BUY_LIST_MAX_ROWS: u64 = 500;

/// One wanted card printing on the shopping list.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct BuyListCard {
    /// The printing's external (Scryfall) id, as the catalog exposes it.
    pub card_id: String,
    pub name: String,
    pub set_code: String,
    pub collector_number: String,
    /// Regular copies wanted.
    pub quantity: i32,
    /// Foil copies wanted.
    pub foil_quantity: i32,
    /// TCGplayer product id of the printing, the key its mass-entry page takes; `null`
    /// when TCGplayer doesn't list it, in which case a client falls back to the name +
    /// set + collector number.
    pub tcgplayer_id: Option<i32>,
}

/// One wanted sealed product on the shopping list.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct BuyListProduct {
    /// The product's external id — its TCGplayer product id, as the catalog exposes it.
    pub product_id: String,
    pub name: String,
    pub quantity: i32,
    pub foil_quantity: i32,
}

/// The shopping list: the wanted cards the request's filters match, and — only for an
/// unfiltered request, a "buy the whole list" — every wanted sealed product too.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct BuyList {
    pub cards: Vec<BuyListCard>,
    pub products: Vec<BuyListProduct>,
    /// Card rows the filters matched, including any beyond the cap.
    pub total_cards: u64,
    /// Sealed-product rows on the wish list (`0` when the request was filtered).
    pub total_products: u64,
    /// `true` when either list was cut at the cap; the totals say by how much.
    pub truncated: bool,
}

/// The narrow row the card query drains: the listing's columns a bulk-entry line needs.
/// Every card column is nullable because the holdings query is a LEFT JOIN — a row whose
/// card is gone (a catalog re-import) comes back as NULLs and is skipped, as the list does.
#[derive(Debug, FromQueryResult)]
pub(crate) struct BuyListCardRow {
    external_id: Option<String>,
    name: Option<String>,
    set_code: Option<String>,
    collector_number: Option<String>,
    quantity: i32,
    foil_quantity: i32,
    tcgplayer_id: Option<i32>,
}

/// Run a holdings list query (a twin's own `collection_query`/`wishlist_query` output,
/// filters and sort untouched) narrowed to the shopping-list columns, capped at
/// [`BUY_LIST_MAX_ROWS`], and count the rows it would have matched uncapped. The twin
/// passes its own count columns, as [`super::narrow_export_statement`] has it, so the
/// projection can't drift between surfaces.
pub(crate) async fn load_buy_list_cards<E, C>(
    db: &DatabaseConnection,
    query: SelectTwo<E, card::Entity>,
    quantity: C,
    foil_quantity: C,
) -> Result<(Vec<BuyListCard>, u64), AppError>
where
    E: EntityTrait,
    E::Model: Send + Sync,
    C: ColumnTrait,
{
    let total = count_statement(db, query.clone()).await?;
    let statement = query
        .select_only()
        .column_as(card::Column::ExternalId, "external_id")
        .column_as(card::Column::Name, "name")
        .column_as(card::Column::SetCode, "set_code")
        .column_as(card::Column::CollectorNumber, "collector_number")
        .column_as(quantity, "quantity")
        .column_as(foil_quantity, "foil_quantity")
        .column_as(card::Column::TcgplayerId, "tcgplayer_id")
        .limit(BUY_LIST_MAX_ROWS)
        .into_query();
    let rows = BuyListCardRow::find_by_statement(db.get_database_backend().build(&statement))
        .all(db)
        .await?;
    Ok((rows.into_iter().filter_map(card_row).collect(), total))
}

/// `COUNT(*)` over the (filtered, unsorted-irrelevant) holdings query, so the wire can
/// say how many rows the cap cut.
async fn count_statement<E>(
    db: &DatabaseConnection,
    query: SelectTwo<E, card::Entity>,
) -> Result<u64, AppError>
where
    E: EntityTrait,
    E::Model: Send + Sync,
{
    use sea_orm::PaginatorTrait;
    Ok(query.count(db).await?)
}

fn card_row(row: BuyListCardRow) -> Option<BuyListCard> {
    Some(BuyListCard {
        card_id: row.external_id?,
        name: row.name?,
        set_code: row.set_code?,
        collector_number: row.collector_number?,
        quantity: row.quantity,
        foil_quantity: row.foil_quantity,
        tcgplayer_id: row.tcgplayer_id,
    })
}

/// Shape a page of wanted sealed products into shopping-list rows.
pub(crate) fn product_rows(entries: Vec<ProductHoldingEntry>) -> Vec<BuyListProduct> {
    entries
        .into_iter()
        .map(|entry| BuyListProduct {
            product_id: entry.product.id,
            name: entry.product.name,
            quantity: entry.quantity,
            foil_quantity: entry.foil_quantity,
        })
        .collect()
}

/// Assemble the wire shape; `truncated` is derived from the totals, never set by hand.
pub(crate) fn build_buy_list(
    cards: Vec<BuyListCard>,
    total_cards: u64,
    products: Vec<BuyListProduct>,
    total_products: u64,
) -> BuyList {
    let truncated = (cards.len() as u64) < total_cards || (products.len() as u64) < total_products;
    BuyList {
        cards,
        products,
        total_cards,
        total_products,
        truncated,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(n: i32) -> BuyListCard {
        BuyListCard {
            card_id: format!("c{n}"),
            name: format!("Card {n}"),
            set_code: "tst".into(),
            collector_number: n.to_string(),
            quantity: 1,
            foil_quantity: 0,
            tcgplayer_id: None,
        }
    }

    #[test]
    fn truncation_is_read_off_the_totals() {
        let whole = build_buy_list(vec![card(1), card(2)], 2, vec![], 0);
        assert!(!whole.truncated);
        let cut = build_buy_list(vec![card(1)], 3, vec![], 0);
        assert!(cut.truncated);
        assert_eq!(cut.total_cards, 3);
        let products_cut = build_buy_list(vec![], 0, vec![], 1);
        assert!(products_cut.truncated);
    }

    #[test]
    fn a_row_whose_card_is_gone_is_skipped() {
        let gone = BuyListCardRow {
            external_id: None,
            name: None,
            set_code: None,
            collector_number: None,
            quantity: 2,
            foil_quantity: 0,
            tcgplayer_id: None,
        };
        assert!(card_row(gone).is_none());
        let present = BuyListCardRow {
            external_id: Some("sf-1".into()),
            name: Some("Sol Ring".into()),
            set_code: Some("cmm".into()),
            collector_number: Some("410".into()),
            quantity: 1,
            foil_quantity: 1,
            tcgplayer_id: Some(500123),
        };
        let row = card_row(present).expect("a present card maps");
        assert_eq!(row.card_id, "sf-1");
        assert_eq!(row.tcgplayer_id, Some(500123));
    }
}
