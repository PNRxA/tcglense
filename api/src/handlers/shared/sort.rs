//! Shared card-list sorting: the user-facing `sort`/`dir` vocabulary and the query
//! ordering it maps to. Reused by the public catalog card lists and the
//! authenticated collection list so both order cards identically.

use sea_orm::{
    Order, QueryOrder,
    sea_query::{Expr, NullOrdering, SimpleExpr},
};

use crate::entities::card;
use crate::error::AppError;
use crate::scryfall::search::{Direction, RARITIES, SortKey};

/// A user-facing card-list sort key. Maps to one or more `card` columns; fields
/// that aren't lexically ordered (rarity, price) sort on a derived expression so
/// the order is meaningful rather than alphabetical/string-wise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortField {
    /// Collector number (numeric run first, then the raw string) — a set
    /// listing's natural order.
    Number,
    Name,
    Rarity,
    Released,
    /// Mana value (converted mana cost).
    Cmc,
    /// USD market price.
    Price,
    /// Set code (then collector number).
    Set,
    /// Colour-count rank (colourless, mono, multi).
    Color,
    Power,
    Toughness,
    /// EDHREC popularity rank (ascending = most popular).
    Edhrec,
    /// EUR market price.
    Eur,
    /// MTGO ticket price.
    Tix,
    Artist,
}

impl SortField {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        Ok(match value {
            "number" | "collector" => SortField::Number,
            "name" => SortField::Name,
            "rarity" => SortField::Rarity,
            "released" | "date" => SortField::Released,
            "cmc" | "mv" => SortField::Cmc,
            "price" | "usd" => SortField::Price,
            "set" => SortField::Set,
            "color" | "colors" => SortField::Color,
            "power" | "pow" => SortField::Power,
            "toughness" | "tou" => SortField::Toughness,
            "edhrec" => SortField::Edhrec,
            "eur" => SortField::Eur,
            "tix" => SortField::Tix,
            "artist" => SortField::Artist,
            other => return Err(AppError::Validation(format!("unknown sort '{other}'"))),
        })
    }

    /// The direction to use when a caller names a field but no `dir`. Newest,
    /// priciest and rarest first read more usefully than the lexical-ascending
    /// default for those fields.
    pub(crate) fn default_dir(self) -> SortDir {
        match self {
            SortField::Number
            | SortField::Name
            | SortField::Cmc
            | SortField::Set
            | SortField::Color
            | SortField::Power
            | SortField::Toughness
            | SortField::Artist
            // Ascending EDHREC rank = most popular first.
            | SortField::Edhrec => SortDir::Asc,
            SortField::Rarity
            | SortField::Released
            | SortField::Price
            | SortField::Eur
            | SortField::Tix => SortDir::Desc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortDir {
    Asc,
    Desc,
}

impl SortDir {
    pub(crate) fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "asc" => Ok(SortDir::Asc),
            "desc" => Ok(SortDir::Desc),
            other => Err(AppError::Validation(format!(
                "unknown sort direction '{other}'"
            ))),
        }
    }

    pub(crate) fn order(self) -> Order {
        match self {
            SortDir::Asc => Order::Asc,
            SortDir::Desc => Order::Desc,
        }
    }
}

impl From<SortKey> for SortField {
    fn from(key: SortKey) -> Self {
        match key {
            SortKey::Name => SortField::Name,
            SortKey::Set => SortField::Set,
            SortKey::Released => SortField::Released,
            SortKey::Rarity => SortField::Rarity,
            SortKey::Color => SortField::Color,
            SortKey::Cmc => SortField::Cmc,
            SortKey::Power => SortField::Power,
            SortKey::Toughness => SortField::Toughness,
            SortKey::Usd => SortField::Price,
            SortKey::Eur => SortField::Eur,
            SortKey::Tix => SortField::Tix,
            SortKey::Edhrec => SortField::Edhrec,
            SortKey::Artist => SortField::Artist,
            SortKey::Number => SortField::Number,
        }
    }
}

impl From<Direction> for SortDir {
    fn from(dir: Direction) -> Self {
        match dir {
            Direction::Asc => SortDir::Asc,
            Direction::Desc => SortDir::Desc,
        }
    }
}

/// Apply the requested ordering to a card query, ending with a stable `id`
/// tiebreaker so pagination is deterministic across pages even when the chosen
/// field has ties. `group_by_set` keeps each set's cards contiguous (used by the
/// related-sets view in collector-number order); it only makes sense alongside
/// the `Number` field, where a per-set grouping is wanted instead of a single
/// flat run. Rarity and price sort on a derived expression with unknown/missing
/// values pushed last regardless of direction.
/// Generic over the query type so it applies to a plain card `Select` (the catalog
/// lists) or a joined query — e.g. the collection list's `SelectTwo` of holdings +
/// cards — reusing one card-sort implementation for both. Every ordering column is
/// entity-qualified (or a `cards`-only bare column in the derived expressions), so
/// it stays unambiguous under a join.
pub(crate) fn apply_card_sort<Q: QueryOrder>(
    query: Q,
    field: SortField,
    dir: SortDir,
    group_by_set: bool,
) -> Q {
    let mut query = if group_by_set {
        query.order_by_asc(card::Column::SetCode)
    } else {
        query
    };
    query = match field {
        SortField::Number => query
            .order_by_with_nulls(card::Column::CollectorNumberInt, dir.order(), NullOrdering::Last)
            .order_by(card::Column::CollectorNumber, dir.order()),
        // Preserve the previous all-cards tiebreak (set, then collector number)
        // so the default listing order is unchanged.
        SortField::Name => query
            .order_by(card::Column::Name, dir.order())
            .order_by_asc(card::Column::SetCode)
            .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last),
        SortField::Rarity => query
            .order_by_with_nulls(rarity_rank_expr(), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Released => query
            .order_by_with_nulls(card::Column::ReleasedAt, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Cmc => query
            .order_by_with_nulls(card::Column::Cmc, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        // Fall back to the foil price when there's no regular USD price, so
        // foil-only printings sort by what they actually cost instead of being
        // parked as unpriced (matches the browse tiles' displayed price).
        SortField::Price => query
            .order_by_with_nulls(
                price_real_expr(&["price_usd", "price_usd_foil"]),
                dir.order(),
                NullOrdering::Last,
            )
            .order_by_asc(card::Column::Name),
        SortField::Set => query
            .order_by(card::Column::SetCode, dir.order())
            .order_by_with_nulls(card::Column::CollectorNumberInt, Order::Asc, NullOrdering::Last),
        SortField::Color => query
            .order_by_with_nulls(color_rank_expr(), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Power => query
            .order_by_with_nulls(numeric_col_expr("power"), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Toughness => query
            .order_by_with_nulls(numeric_col_expr("toughness"), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Edhrec => query
            .order_by_with_nulls(card::Column::EdhrecRank, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Eur => query
            .order_by_with_nulls(price_real_expr(&["price_eur"]), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Tix => query
            .order_by_with_nulls(price_real_expr(&["price_tix"]), dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
        SortField::Artist => query
            .order_by_with_nulls(card::Column::Artist, dir.order(), NullOrdering::Last)
            .order_by_asc(card::Column::Name),
    };
    query.order_by_asc(card::Column::Id)
}

/// SQL expression mapping `rarity` to its canonical low→high ordinal, reusing the
/// search grammar's rarity ranking (`scryfall::search::RARITIES`) so the sort and
/// the `r>=`/`r<` filters stay in lockstep. Unknown/missing rarities map to NULL
/// so `NULLS LAST` parks them at the end in either direction. The interpolated
/// values are fixed lowercase rarity names and integer ranks — never user input.
fn rarity_rank_expr() -> SimpleExpr {
    let arms: String = RARITIES
        .iter()
        .enumerate()
        .map(|(rank, name)| format!("WHEN '{name}' THEN {rank}"))
        .collect::<Vec<_>>()
        .join(" ");
    Expr::cust(format!("CASE IFNULL(rarity, '') {arms} ELSE NULL END"))
}

/// SQL expression giving a card's sort price: the first non-empty value among
/// `cols` (in order), cast to a real number. Each column is NULL/empty-guarded
/// (so `''` isn't treated as `0`) and the guarded values are `COALESCE`d, so a
/// card priced only in a later column — e.g. a foil-only printing with no regular
/// `price_usd` — still sorts by that price rather than being treated as unpriced.
/// When every column is NULL/empty the result is NULL, so `NULLS LAST` keeps
/// truly-unpriced cards at the end in either direction. `cols` are fixed column
/// names, never user input.
fn price_real_expr(cols: &[&str]) -> SimpleExpr {
    let arms: Vec<String> = cols
        .iter()
        .map(|col| {
            format!("CASE WHEN {col} IS NULL OR {col} = '' THEN NULL ELSE CAST({col} AS REAL) END")
        })
        .collect();
    // SQLite's COALESCE needs ≥2 arguments; a single column is emitted bare.
    let expr = match arms.as_slice() {
        [single] => single.clone(),
        _ => format!("COALESCE({})", arms.join(", ")),
    };
    Expr::cust(expr)
}

/// Colour-count rank for `order:color`: colourless (0) first, then mono, then
/// multicoloured. `colors` is a cards-only column, so it's unambiguous under a join.
fn color_rank_expr() -> SimpleExpr {
    Expr::cust(
        "CASE WHEN colors IS NULL OR colors = '' THEN 0 \
         ELSE LENGTH(colors) - LENGTH(REPLACE(colors, ',', '')) + 1 END",
    )
}

/// Numeric value of a power/toughness-style text column for sorting, or NULL when
/// non-numeric (so `NULLS LAST` parks `*`/`X`/absent values at the end). `col` is a
/// fixed, cards-only column name — never user input.
fn numeric_col_expr(col: &str) -> SimpleExpr {
    Expr::cust(format!(
        "CASE WHEN {col} GLOB '[0-9]*' AND {col} NOT GLOB '*[^0-9]*' \
         THEN CAST({col} AS REAL) ELSE NULL END"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::card_model;
    use sea_orm::EntityTrait;

    /// A minimal, insertable card row whose only meaningful fields for the sort
    /// tests are its id and the two USD price columns.
    fn test_card(id: i32, usd: Option<&str>, usd_foil: Option<&str>) -> card::Model {
        card::Model {
            price_usd: usd.map(str::to_string),
            price_usd_foil: usd_foil.map(str::to_string),
            ..card_model(id)
        }
    }

    /// The price sort falls back to the foil price when a card has no regular USD
    /// price (some printings are foil-only) but still prefers the regular price
    /// when present, with unpriced cards parked last in either direction.
    #[tokio::test]
    async fn price_sort_falls_back_to_foil_when_no_regular_usd() {
        use sea_orm::{ActiveModelTrait, IntoActiveModel};

        let db = crate::test_support::migrated_memory_db().await;

        // 1: regular $5 (also a $50 foil — the regular price must win over it).
        // 2: foil-only $20.  3: foil-only $1.  4: fully unpriced.
        // 5: empty-string regular price + $8 foil — the `= ''` guard must treat the
        //    empty regular price as absent and fall through to the foil price.
        for c in [
            test_card(1, Some("5.00"), Some("50.00")),
            test_card(2, None, Some("20.00")),
            test_card(3, None, Some("1.00")),
            test_card(4, None, None),
            test_card(5, Some(""), Some("8.00")),
        ] {
            c.into_active_model().insert(&db).await.expect("insert card");
        }

        let ids = |rows: Vec<card::Model>| rows.iter().map(|r| r.id).collect::<Vec<_>>();

        // Effective price = regular USD (when non-empty), else foil. Desc: 2($20),
        // 5($8 via foil), 1($5), 3($1), then the unpriced 4 last. Card 1 sorts on
        // its $5 regular price, not its $50 foil — the fallback only applies when
        // the regular price is missing or empty.
        let desc = apply_card_sort(card::Entity::find(), SortField::Price, SortDir::Desc, false)
            .all(&db)
            .await
            .expect("query desc");
        assert_eq!(ids(desc), vec![2, 5, 1, 3, 4]);

        // Asc mirrors it, with the unpriced card still parked last (NULLS LAST).
        let asc = apply_card_sort(card::Entity::find(), SortField::Price, SortDir::Asc, false)
            .all(&db)
            .await
            .expect("query asc");
        assert_eq!(ids(asc), vec![3, 1, 5, 2, 4]);
    }
}
