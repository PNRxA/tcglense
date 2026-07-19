//! The machine-readable OpenAPI 3.1 description of the public HTTP API (issue #284).
//!
//! [`ApiDoc`] is a [`utoipa::OpenApi`] document assembled at compile time from the
//! `#[utoipa::path]` annotations on the handlers and the `#[derive(utoipa::ToSchema)]`
//! on the wire DTOs. It's served as raw JSON at `GET /api/openapi.json` (a public,
//! CDN-cacheable route — see [`crate::handlers::openapi`]) and rendered as an interactive
//! reference by the SPA at `/docs` (`web/src/views/DocsView.vue`, `@scalar/api-reference`).
//!
//! Coverage is every JSON data endpoint a public or API-key consumer can call: the read
//! catalog (cards + sealed products), the authenticated collection / wish-list / decks
//! surfaces, the handle-addressed public-sharing reads, and API-key management. The routes
//! deliberately left out — binary image proxies, the SPA's session/sign-in flow, sitemaps,
//! the opt-in dataset mirror — are enumerated (with reasons) in the `coverage_drift` test's
//! allow-list below, which fails CI if a new route slips out of both the doc and that list.
//! [`SecurityAddon`] registers two bearer schemes: `api_key` (a long-lived
//! `Authorization: Bearer tcgl_...` key — authenticates the collection/wish-list/decks reads
//! and writes) and `session` (a short-lived sign-in JWT — required to *manage* keys, since a
//! key cannot manage keys).

use utoipa::{
    Modify, OpenApi,
    openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme},
};

/// Registers the two bearer security schemes so the authenticated endpoints can
/// reference them and Scalar renders an "Authorize" box: `api_key` (a personal API
/// key) for the collection/wish-list surface, and `session` (a sign-in JWT) for the
/// key-management endpoints — which reject an API key, so they must advertise the
/// session scheme rather than `api_key`.
pub struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi
            .components
            .get_or_insert_with(utoipa::openapi::Components::default);
        components.add_security_scheme(
            "api_key",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("tcgl_...")
                    .description(Some(
                        "A personal API key, presented as `Authorization: Bearer tcgl_...`. \
                         Mint one from your profile page. Authenticates the collection and \
                         wish-list endpoints (but cannot manage keys — see `session`).",
                    ))
                    .build(),
            ),
        );
        components.add_security_scheme(
            "session",
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .bearer_format("JWT")
                    .description(Some(
                        "A session access token (a short-lived JWT obtained by signing in), \
                         presented as `Authorization: Bearer <jwt>`. Required to manage API \
                         keys — an API key cannot mint, list, or revoke keys.",
                    ))
                    .build(),
            ),
        );
    }
}

/// The OpenAPI 3.1 document for the public TCGLense API.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "TCGLense API",
        version = env!("CARGO_PKG_VERSION"),
        description = "Public HTTP JSON API for TCGLense: a game-agnostic card catalog \
(cards + sealed products, MTG first) with singles/sealed price history, plus per-user \
collection and wish-list holdings (the wish list also tracks wanted sealed products). \
Read endpoints are unauthenticated and CDN-cacheable; \
the collection, wish-list, and API-key endpoints authenticate with a personal API key \
(`Authorization: Bearer tcgl_...`). Sign in and create a key from your profile page \
(Profile → API keys) — the token is shown only once, so copy it when it's created.",
        license(name = "See repository")
    ),
    paths(
        // --- Cards ---
        crate::handlers::catalog::list_games,
        crate::handlers::catalog::list_sets,
        crate::handlers::catalog::get_set,
        crate::handlers::catalog::list_set_cards,
        crate::handlers::catalog::list_set_drops,
        crate::handlers::catalog::list_cards,
        crate::handlers::catalog::get_card,
        crate::handlers::catalog::card_prices,
        crate::handlers::catalog::card_prints,
        // --- Sealed products ---
        crate::handlers::catalog::list_products,
        crate::handlers::catalog::product_facets,
        crate::handlers::catalog::get_product,
        crate::handlers::catalog::product_prices,
        crate::handlers::catalog::product_contents,
        crate::handlers::catalog::product_containers,
        // --- Collection ---
        crate::handlers::collection::list_collection,
        crate::handlers::collection::collection_summary,
        crate::handlers::collection::get_collection_entry,
        crate::handlers::collection::set_collection_entry,
        crate::handlers::collection::list_collection_products,
        crate::handlers::collection::list_collection_product_sets,
        crate::handlers::collection::get_collection_product_entry,
        crate::handlers::collection::set_collection_product_entry,
        crate::handlers::collection::collection_product_summary,
        // --- Wish list ---
        crate::handlers::wishlist::list_wishlist,
        crate::handlers::wishlist::wishlist_summary,
        crate::handlers::wishlist::get_wishlist_entry,
        crate::handlers::wishlist::set_wishlist_entry,
        crate::handlers::wishlist::list_wishlist_products,
        crate::handlers::wishlist::list_wishlist_product_sets,
        crate::handlers::wishlist::get_wishlist_product_entry,
        crate::handlers::wishlist::set_wishlist_product_entry,
        crate::handlers::wishlist::wishlist_product_summary,
        // --- API keys ---
        crate::handlers::api_keys::create_api_key,
        crate::handlers::api_keys::list_api_keys,
        crate::handlers::api_keys::revoke_api_key,
        // --- Cards: set sub-types, name autocomplete, import status, sealed membership, scanner ---
        crate::handlers::catalog::list_set_subtypes,
        crate::handlers::catalog::card_names,
        crate::handlers::catalog::ingest_status,
        crate::handlers::catalog::card_sealed,
        crate::handlers::catalog::scan_cards,
        // --- Sealed products: contained cards + their display sections ---
        crate::handlers::catalog::product_cards,
        crate::handlers::catalog::product_card_sections,
        // --- Collection: per-set views, valuation, movers, batch counts, import/sync/export, sharing toggle ---
        crate::handlers::collection::collection_sets,
        crate::handlers::collection::collection_set_drops,
        crate::handlers::collection::collection_set_subtypes,
        crate::handlers::collection::collection_value_history,
        crate::handlers::collection::collection_movers,
        crate::handlers::collection::owned_counts,
        crate::handlers::collection::collection_product_counts,
        crate::handlers::collection::export_collection,
        crate::handlers::collection::import_collection,
        crate::handlers::collection::import_collection_csv,
        crate::handlers::collection::get_import_job,
        crate::handlers::collection::get_collection_source,
        crate::handlers::collection::save_collection_source,
        crate::handlers::collection::delete_collection_source,
        crate::handlers::collection::sync_collection_source,
        crate::handlers::sharing::get_collection_visibility,
        crate::handlers::sharing::set_collection_visibility,
        // --- Wish list: per-set views + batch wanted-counts (cards + sealed products) ---
        crate::handlers::wishlist::wishlist_sets,
        crate::handlers::wishlist::wishlist_set_drops,
        crate::handlers::wishlist::wishlist_set_subtypes,
        crate::handlers::wishlist::wishlist_counts,
        crate::handlers::wishlist::wishlist_product_counts,
        // Wish-list public-sharing toggle (issue #493).
        crate::handlers::sharing::get_wishlist_visibility,
        crate::handlers::sharing::set_wishlist_visibility,
        // --- Decks (issues #363/#389): decks, import/export, folders, sections, cards ---
        crate::handlers::decks::list_decks,
        crate::handlers::decks::needed_cards,
        crate::handlers::decks::create_deck,
        crate::handlers::decks::import_deck,
        crate::handlers::decks::get_deck,
        crate::handlers::decks::update_deck,
        crate::handlers::decks::delete_deck,
        crate::handlers::decks::move_deck_to_folder,
        crate::handlers::decks::set_deck_visibility,
        crate::handlers::decks::list_folders,
        crate::handlers::decks::create_folder,
        crate::handlers::decks::update_folder,
        crate::handlers::decks::delete_folder,
        crate::handlers::decks::create_section,
        crate::handlers::decks::reorder_sections,
        crate::handlers::decks::update_section,
        crate::handlers::decks::delete_section,
        crate::handlers::decks::set_deck_card,
        crate::handlers::decks::move_deck_card,
        crate::handlers::decks::change_deck_card_printing,
        crate::handlers::decks::export_deck,
        // --- Public sharing (issues #361/#362/#363): handle-keyed public collection + decks ---
        crate::handlers::sharing::public_profile,
        crate::handlers::sharing::public_list,
        crate::handlers::sharing::public_summary,
        crate::handlers::sharing::public_sets,
        crate::handlers::sharing::public_set_drops,
        crate::handlers::sharing::public_set_subtypes,
        crate::handlers::sharing::public_products,
        crate::handlers::sharing::public_product_summary,
        crate::handlers::sharing::public_product_sets,
        crate::handlers::sharing::public_owned_counts,
        crate::handlers::sharing::public_decks,
        crate::handlers::sharing::public_deck,
        // --- Public wish lists (issue #493): handle-keyed read-only mirror of the above ---
        crate::handlers::sharing::public_wishlist_list,
        crate::handlers::sharing::public_wishlist_summary,
        crate::handlers::sharing::public_wishlist_sets,
        crate::handlers::sharing::public_wishlist_set_drops,
        crate::handlers::sharing::public_wishlist_set_subtypes,
        crate::handlers::sharing::public_wishlist_products,
        crate::handlers::sharing::public_wishlist_product_summary,
        crate::handlers::sharing::public_wishlist_product_sets,
        crate::handlers::sharing::public_wishlist_owned_counts,
    ),
    components(schemas(
        // Leaf DTOs reachable from this module; the generic `Page<T>` / `DataBody<T>`
        // wrappers and the catalog DTOs living in private submodules are collected
        // automatically from the annotated path response bodies.
        crate::catalog::Game,
        crate::handlers::shared::CardResponse,
        crate::handlers::shared::dto::PricesResponse,
        crate::handlers::shared::dto::CardFaceResponse,
        crate::handlers::shared::CollectionEntry,
        crate::handlers::shared::CollectionQuantities,
        crate::handlers::shared::CollectionSummary,
        crate::handlers::shared::SetQuantitiesRequest,
        crate::handlers::api_keys::CreateApiKeyRequest,
        crate::handlers::api_keys::CreatedApiKey,
        crate::handlers::api_keys::ApiKeyInfo,
        crate::handlers::api_keys::ApiKeyList,
    )),
    modifiers(&SecurityAddon),
    tags(
        (name = "Cards", description = "Card catalog: games, sets, cards, prices."),
        (name = "Sealed products", description = "Sealed products (boxes, bundles, decks) + prices + contents."),
        (name = "Collection", description = "The signed-in user's owned-card holdings (requires an API key)."),
        (name = "Wish list", description = "The signed-in user's wanted cards + sealed products (requires an API key)."),
        (name = "Decks", description = "The signed-in user's decks, folders, and sections (requires an API key)."),
        (name = "Public sharing", description = "Read-only, handle-addressed views of a user's public collection + decks (no authentication)."),
        (name = "API keys", description = "Mint, list, and revoke the personal API keys that authenticate the public API."),
    )
)]
pub struct ApiDoc;

#[cfg(test)]
mod tests {
    use super::*;

    /// A malformed `#[openapi(...)]` attribute or an un-`ToSchema`-able DTO surfaces
    /// as a panic when the document is materialized; building it under test catches
    /// that at `cargo test` time rather than only when the route is first hit.
    #[test]
    fn openapi_spec_builds() {
        let doc = ApiDoc::openapi();
        // The security scheme modifier ran and the paths are present.
        let json = serde_json::to_value(&doc).expect("spec serializes to JSON");
        assert_eq!(
            json["openapi"].as_str().unwrap_or_default().get(0..3),
            Some("3.1")
        );
        assert!(
            json["paths"]["/api/games/{game}/cards"].is_object(),
            "list_cards path is documented"
        );
        assert!(
            json["components"]["securitySchemes"]["api_key"].is_object(),
            "api_key security scheme is registered"
        );
        assert!(
            json["components"]["securitySchemes"]["session"].is_object(),
            "session security scheme is registered (for key management)"
        );
        // The formerly-undocumented surfaces now appear (decks + handle-keyed sharing).
        assert!(
            json["paths"]["/api/decks/{game}/{deck_id}"].is_object(),
            "the deck detail path is documented"
        );
        assert!(
            json["paths"]["/api/u/{handle}/decks/{deck_id}"].is_object(),
            "the public deck path is documented"
        );
        // utoipa auto-collects the response/request DTO schemas reachable from those paths;
        // spot-check one leaf per newly-documented surface so a dangling $ref can't slip.
        for schema in [
            "DeckDetail",
            "DeckResponse",
            "ImportJobResponse",
            "CollectionMovers",
            "PublicProfile",
            "StatusResponse",
        ] {
            assert!(
                json["components"]["schemas"][schema].is_object(),
                "schema `{schema}` should be present in components.schemas"
            );
        }
    }

    /// The Scalar `/docs` sidebar labels each operation with its `summary`, which utoipa
    /// takes from the **first line** of the handler's doc comment (everything up to the
    /// first blank `///` line becomes the summary; the rest is the description). A handler
    /// whose doc comment is one long `` `GET /api/... -> …` `` paragraph — with no short
    /// title line — turns that whole paragraph into the sidebar label. Guard against it:
    /// every operation must carry a concise title summary. Fix a failure by prefixing the
    /// handler's doc comment with a short `/// Title` line followed by a blank `///` line.
    #[test]
    fn every_operation_has_a_concise_summary() {
        let json = serde_json::to_value(ApiDoc::openapi()).expect("spec serializes to JSON");
        let mut bad = Vec::new();
        for (path, item) in json["paths"].as_object().expect("spec has a paths object") {
            for (method, op) in item.as_object().expect("path item is an object") {
                let summary = op.get("summary").and_then(|v| v.as_str()).unwrap_or("");
                // A real title is short and is not the `METHOD /path -> …` description form.
                if summary.is_empty() || summary.chars().count() > 80 || summary.starts_with('`') {
                    bad.push(format!("{} {path} -> {summary:?}", method.to_uppercase()));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "these operations lack a concise summary — prefix the handler doc comment with a \
             short `/// Title` line + a blank `///` line so the /docs sidebar reads well:\n{}",
            bad.join("\n")
        );
    }
}

/// Guards that the hand-maintained [`ApiDoc`] stays in sync with the router: every
/// `.route(...)` in `router.rs` must be either documented in [`ApiDoc`] or explicitly, and
/// with a stated reason, allow-listed. This is the safety net that would have caught the
/// decks/public-sharing/collection gaps this module was overhauled to close.
#[cfg(test)]
mod coverage_drift {
    use super::*;
    use std::collections::BTreeSet;

    /// `api/src/router.rs`, embedded at compile time. `include_str!` resolves relative to
    /// THIS file (`api/src/openapi.rs`), so the check runs from any working directory and
    /// recompiles whenever the router changes.
    const ROUTER_SRC: &str = include_str!("router.rs");

    /// Routes deliberately absent from the OpenAPI document, each with the reason it stays
    /// undocumented. Adding a `.route(...)` to `router.rs` without either a `#[utoipa::path]`
    /// wired into [`ApiDoc`] *or* an entry here fails
    /// `every_route_is_documented_or_allow_listed`. The `allow_list_*` guards keep this list
    /// from rotting. Everything a public/API-key consumer calls as JSON is documented; only
    /// these infrastructure / binary / SPA-session routes are intentionally excluded.
    const INTENTIONALLY_UNDOCUMENTED: &[(&str, &str)] = &[
        // --- Health probes / SPA runtime config: infra, not part of the public data API. ---
        ("/api/health", "liveness probe; not a data endpoint"),
        (
            "/api/ready",
            "database readiness probe; not a data endpoint",
        ),
        (
            "/api/config",
            "SPA bootstrap config (Turnstile site key); internal to the web app",
        ),
        (
            "/api/currencies",
            "SPA display exchange rates; internal to the web app",
        ),
        // --- Auth & account: the SPA's session/cookie flow. API-key *management*
        //     (`/api/auth/api-keys`) IS documented; everything else is sign-in plumbing. ---
        (
            "/api/auth/register",
            "email-first registration; SPA session flow",
        ),
        (
            "/api/auth/complete-registration",
            "email-first registration; SPA session flow",
        ),
        (
            "/api/auth/login",
            "sign-in; issues session JWT + refresh cookie, SPA session flow",
        ),
        (
            "/api/auth/refresh",
            "refresh-cookie rotation; SPA session flow",
        ),
        ("/api/auth/logout", "session teardown; SPA session flow"),
        ("/api/auth/me", "current-session identity; SPA session flow"),
        (
            "/api/auth/currency",
            "account display preference; SPA session flow",
        ),
        ("/api/auth/username", "handle claim; SPA session flow"),
        (
            "/api/auth/username/available",
            "handle availability check; SPA session flow",
        ),
        (
            "/api/auth/verify-email",
            "single-use email token; SPA session flow",
        ),
        (
            "/api/auth/resend-verification",
            "single-use email token; SPA session flow",
        ),
        (
            "/api/auth/forgot-password",
            "single-use email token; SPA session flow",
        ),
        (
            "/api/auth/reset-password",
            "single-use email token; SPA session flow",
        ),
        // --- Binary image proxies: return image/SVG bytes, not JSON. ---
        (
            "/api/games/{game}/sets/{code}/icon",
            "binary set-icon image, not JSON",
        ),
        (
            "/api/games/{game}/cards/{id}/image",
            "binary card image, not JSON",
        ),
        (
            "/api/games/{game}/products/{id}/image",
            "binary product image, not JSON",
        ),
        // --- The spec document itself. ---
        ("/api/openapi.json", "the OpenAPI document itself"),
        // --- Sitemaps: XML for crawlers — root canonical form plus the `/api/` aliases. ---
        ("/sitemap.xml", "XML sitemap index for crawlers, not JSON"),
        (
            "/sitemaps/{name}",
            "XML child sitemap for crawlers, not JSON",
        ),
        ("/api/sitemap.xml", "`/api/` alias of the XML sitemap index"),
        (
            "/api/sitemaps/{name}",
            "`/api/` alias of the XML child sitemap",
        ),
        // --- Optional dataset mirror (off by default): re-serves raw upstream datasets. ---
        (
            "/api/mirror/scryfall/bulk-data",
            "opt-in dataset mirror; not the public data API",
        ),
        ("/api/mirror/scryfall/sets", "opt-in dataset mirror"),
        ("/api/mirror/scryfall/file/{kind}", "opt-in dataset mirror"),
        (
            "/api/mirror/scryfall/sld-drops",
            "opt-in dataset mirror (Secret Lair drop snapshot)",
        ),
        (
            "/api/mirror/mtgjson/AllPrintings.json.gz",
            "opt-in dataset mirror (binary blob)",
        ),
        (
            "/api/mirror/tcgcsv/{*path}",
            "opt-in dataset mirror (proxy passthrough)",
        ),
        (
            "/api/mirror/fingerprints/{game}",
            "opt-in scanner fingerprint index",
        ),
        // --- The WEB_ROOT catch-all turning an unmatched `/api/*` into a JSON 404. ---
        (
            "/api/{*rest}",
            "combined-image 404 catch-all, not a real endpoint",
        ),
    ];

    /// Every `.route("<path>", …)` literal in `router.rs`, deduped. The regex captures only
    /// the FIRST string literal after `.route(`, so `get(h).post(h2)` chaining and any
    /// trailing `.layer(...)` are ignored; `\s*` spans the multi-line `.route(\n  "…",` calls.
    fn router_paths() -> BTreeSet<String> {
        let re = regex::Regex::new(r#"\.route\(\s*"(/[^"]*)""#).expect("valid route regex");
        re.captures_iter(ROUTER_SRC)
            .map(|c| c[1].to_string())
            .collect()
    }

    /// Every path documented in the built OpenAPI doc. utoipa emits `{param}` braces,
    /// byte-identical to axum 0.8, so the keys line up with the router literals directly.
    fn documented_paths() -> BTreeSet<String> {
        let spec = serde_json::to_value(ApiDoc::openapi()).expect("spec serializes to JSON");
        spec["paths"]
            .as_object()
            .expect("spec has a paths object")
            .keys()
            .cloned()
            .collect()
    }

    fn allow_list() -> BTreeSet<String> {
        INTENTIONALLY_UNDOCUMENTED
            .iter()
            .map(|(p, _)| (*p).to_string())
            .collect()
    }

    /// Guard the extractor itself: it must find a known route and a realistic count, so a
    /// broken regex that silently matches nothing can't make the drift check vacuously pass.
    #[test]
    fn router_paths_are_extracted() {
        let paths = router_paths();
        assert!(
            paths.contains("/api/games/{game}/cards"),
            "extractor should find a known route; got {paths:#?}"
        );
        assert!(
            paths.len() > 50,
            "expected the full route table (>50 routes), extracted {} — regex likely broke",
            paths.len()
        );
    }

    /// THE DRIFT GUARD. A new `.route(...)` must be documented in [`ApiDoc`] or listed in
    /// `INTENTIONALLY_UNDOCUMENTED`. Fails with the exact offending paths until one holds.
    #[test]
    fn every_route_is_documented_or_allow_listed() {
        let documented = documented_paths();
        let allowed = allow_list();
        let undocumented: Vec<String> = router_paths()
            .into_iter()
            .filter(|p| !documented.contains(p) && !allowed.contains(p))
            .collect();

        assert!(
            undocumented.is_empty(),
            "these routes in router.rs are neither documented in openapi.rs (add a \
             `#[utoipa::path]` on the handler and list it in `ApiDoc`'s `paths(...)`) nor in \
             the `INTENTIONALLY_UNDOCUMENTED` allow-list (add them there with a reason):\n{}",
            undocumented.join("\n")
        );
    }

    /// Keep the allow-list honest: no entry may name a route that no longer exists in
    /// router.rs (deleted/renamed — drop the entry) or that is now actually documented.
    #[test]
    fn allow_list_has_no_stale_entries() {
        let routes = router_paths();
        let documented = documented_paths();

        let missing: Vec<&str> = INTENTIONALLY_UNDOCUMENTED
            .iter()
            .map(|(p, _)| *p)
            .filter(|p| !routes.contains(*p))
            .collect();
        assert!(
            missing.is_empty(),
            "allow-list names routes that no longer exist in router.rs — remove them:\n{}",
            missing.join("\n")
        );

        let now_documented: Vec<&str> = INTENTIONALLY_UNDOCUMENTED
            .iter()
            .map(|(p, _)| *p)
            .filter(|p| documented.contains(*p))
            .collect();
        assert!(
            now_documented.is_empty(),
            "these allow-listed routes are now documented in openapi.rs — drop them from \
             `INTENTIONALLY_UNDOCUMENTED`:\n{}",
            now_documented.join("\n")
        );
    }

    /// Reverse direction: every documented path must map to a real route, so a typo'd
    /// `#[utoipa::path(path = "...")]` (documenting a route that doesn't exist) can't slip.
    #[test]
    fn documented_paths_all_exist_in_router() {
        let routes = router_paths();
        let phantom: Vec<String> = documented_paths()
            .into_iter()
            .filter(|p| !routes.contains(p))
            .collect();
        assert!(
            phantom.is_empty(),
            "openapi.rs documents paths with no matching `.route(...)` in router.rs \
             (mismatched `#[utoipa::path]` path string?):\n{}",
            phantom.join("\n")
        );
    }
}
