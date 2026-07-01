//! Logic shared between the public catalog handlers ([`crate::handlers::catalog`])
//! and the authenticated collection handlers ([`crate::handlers::collection`]):
//! pagination, card DTOs, game/set/card lookups, card sorting, drop grouping,
//! search compilation, and collection valuation. Extracted so the two handler
//! modules stop re-implementing the same helpers and `collection` no longer imports
//! from its sibling `catalog`.
//!
//! Nothing here may import from `handlers::catalog` or `handlers::collection` — the
//! dependency only ever flows *into* `shared`.

pub(crate) mod dto;
pub(crate) mod grouping;
pub(crate) mod lookup;
pub(crate) mod pagination;
pub(crate) mod search;
pub(crate) mod sort;
pub(crate) mod valuation;

pub(crate) use dto::{CardResponse, stored_faces};
pub(crate) use grouping::{group_into_drops, paginate_buckets, require_drop_table};
pub(crate) use lookup::{load_card, load_group_set_codes, load_set, require_game};
pub(crate) use pagination::{
    DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE, MAX_PAGE_SIZE, Page,
    build_page, resolve_page, trim_query,
};
pub(crate) use search::search_condition;
pub(crate) use sort::{SortDir, SortField, apply_card_sort};
pub(crate) use valuation::Valuation;
