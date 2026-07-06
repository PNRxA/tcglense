//! Logic shared between the public catalog handlers ([`crate::handlers::catalog`])
//! and the authenticated holdings handlers ([`crate::handlers::collection`] +
//! [`crate::handlers::wishlist`]): pagination, card DTOs, game/set/card lookups,
//! card sorting, drop grouping, search compilation, holdings valuation, and the
//! entity-agnostic holdings DTOs/params. Extracted so the handler modules stop
//! re-implementing the same helpers and never import from a sibling.
//!
//! Nothing here may import from `handlers::catalog`, `handlers::collection`, or
//! `handlers::wishlist` — the dependency only ever flows *into* `shared`.

pub(crate) mod dto;
pub(crate) mod grouping;
pub(crate) mod holdings;
pub(crate) mod lookup;
pub(crate) mod pagination;
pub(crate) mod search;
pub(crate) mod sort;
pub(crate) mod valuation;

pub(crate) use dto::{CardResponse, stored_faces};
pub(crate) use grouping::{
    filter_drops_by_title, group_into_drops, paginate_buckets, require_drop_table,
};
pub(crate) use holdings::{
    CollectionDropGroup, CollectionEntry, CollectionQuantities, CollectionSetsResponse,
    CollectionSort, CollectionSummary, ListParams, MAX_OWNED_IDS, OwnedCountsRequest,
    OwnedCountsResponse, SetQuantitiesRequest, SummaryParams, build_collection_sets, copies_expr,
    dedupe_ids, resolve_set_scope, summarize_holdings, validate_quantity,
};
pub(crate) use lookup::{load_card, load_group_set_codes, load_set, require_game};
pub(crate) use pagination::{
    DEFAULT_DROP_PAGE_SIZE, DEFAULT_PAGE_SIZE, DataBody, MAX_DROP_PAGE_SIZE, MAX_PAGE_SIZE, Page,
    build_page, resolve_page, trim_query,
};
pub(crate) use search::search_condition;
pub(crate) use sort::{SortDir, SortField, apply_card_sort};
