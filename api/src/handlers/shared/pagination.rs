//! Pagination primitives shared by the catalog and collection card lists: the
//! page-size bounds, the wire `Page<T>` shape, and the small helpers that resolve
//! + clamp a requested page and trim a search query.

use serde::Serialize;

pub(crate) const DEFAULT_PAGE_SIZE: u64 = 60;
pub(crate) const MAX_PAGE_SIZE: u64 = 200;
/// The by-drop endpoints paginate by *drop* (each drop is a handful of cards), so
/// they use their own smaller default than the per-card lists.
pub(crate) const DEFAULT_DROP_PAGE_SIZE: u64 = 20;
pub(crate) const MAX_DROP_PAGE_SIZE: u64 = 100;
/// Ceiling on the requested page. Far past any real dataset (a maxed-out
/// `page_size` puts page 1M at row 200M), yet small enough that the
/// `offset = page * page_size` multiply downstream (SeaORM's paginator) can't
/// overflow the u64 — which would panic a debug build and wrap to a bogus
/// offset in release. A clamped page behaves like any other past-the-end page:
/// empty `data`, `has_more: false`.
pub(crate) const MAX_PAGE: u64 = 1_000_000;

/// A bare `{ "data": T }` wire envelope — the typed form of the handlers'
/// `json!({ "data": ... })` responses, for endpoints that return a plain
/// (non-paginated) payload rather than a [`Page`].
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct DataBody<T> {
    pub data: T,
}

/// A page of results plus the cursor metadata the SPA needs to paginate.
#[derive(Debug, Serialize, utoipa::ToSchema)]
#[cfg_attr(test, derive(ts_rs::TS), ts(export))]
pub(crate) struct Page<T> {
    pub data: Vec<T>,
    pub page: u64,
    pub page_size: u64,
    pub total: u64,
    pub has_more: bool,
}

impl<T> Page<T> {
    /// Build a page, deriving `has_more` from the cursor position: there is a next
    /// page whenever the rows consumed so far (`page * page_size`) fall short of the
    /// total. Saturating so a huge `page`/`page_size` can't overflow.
    pub(crate) fn new(data: Vec<T>, page: u64, page_size: u64, total: u64) -> Self {
        Page {
            data,
            page,
            page_size,
            total,
            has_more: page.saturating_mul(page_size) < total,
        }
    }
}

/// Build a [`Page`] from already-serialized rows — the generic entry point the
/// handlers use once they've turned their query rows into response DTOs.
pub(crate) fn build_page<T>(data: Vec<T>, page: u64, page_size: u64, total: u64) -> Page<T> {
    Page::new(data, page, page_size, total)
}

/// Resolve a requested (1-based) `page` and clamp the `page_size` against the
/// caller-supplied `default`/`max` bounds. The card and by-drop listings differ
/// only in those two constants, so both go through this. The page is clamped to
/// [`MAX_PAGE`] so a huge `?page` can't overflow the offset multiply downstream.
pub(crate) fn resolve_page(
    page: Option<u64>,
    page_size: Option<u64>,
    default: u64,
    max: u64,
) -> (u64, u64) {
    let page = page.unwrap_or(1).clamp(1, MAX_PAGE);
    let page_size = page_size.unwrap_or(default).clamp(1, max);
    (page, page_size)
}

/// The trimmed search query, or `None` when it's absent or blank — the shared
/// "trim + drop-if-blank" logic for a `?q`/`?set` style param.
pub(crate) fn trim_query(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|q| !q.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_page_clamps_both_bounds() {
        // Zero/absent floor to 1; an absurd page clamps to MAX_PAGE instead of
        // riding into SeaORM's `page * page_size` offset multiply (u64 overflow:
        // debug panic, release wraparound).
        assert_eq!(resolve_page(None, None, 60, 200), (1, 60));
        assert_eq!(resolve_page(Some(0), Some(0), 60, 200), (1, 1));
        assert_eq!(resolve_page(Some(u64::MAX), Some(u64::MAX), 60, 200), (MAX_PAGE, 200));
    }

    #[test]
    fn build_page_derives_has_more() {
        let page = build_page(vec![1, 2, 3], 1, 3, 10);
        assert!(page.has_more, "more rows remain after page 1");
        let page = build_page(vec![1], 4, 3, 10);
        assert!(!page.has_more, "page 4 of 10 rows is the last");
        let page = build_page(Vec::<i32>::new(), 1, 60, 0);
        assert!(!page.has_more);
    }
}
