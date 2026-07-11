//! The machine-readable OpenAPI 3.1 description of the public HTTP API (issue #284).
//!
//! [`ApiDoc`] is a [`utoipa::OpenApi`] document assembled at compile time from the
//! `#[utoipa::path]` annotations on the handlers and the `#[derive(utoipa::ToSchema)]`
//! on the wire DTOs. It's served as raw JSON at `GET /api/openapi.json` (a public,
//! CDN-cacheable route — see [`crate::handlers::openapi`]) and rendered as an interactive
//! reference by the SPA at `/docs` (`web/src/views/DocsView.vue`, `@scalar/api-reference`).
//!
//! Coverage is the read catalog (cards + sealed products) plus the authenticated
//! collection / wish-list / API-key surfaces. [`SecurityAddon`] registers two bearer
//! schemes: `api_key` (a long-lived `Authorization: Bearer tcgl_...` key — authenticates
//! the collection/wish-list reads and writes) and `session` (a short-lived sign-in JWT —
//! required to *manage* keys, since a key cannot manage keys).

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
collection and wish-list holdings. Read endpoints are unauthenticated and CDN-cacheable; \
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
        // --- Collection ---
        crate::handlers::collection::list_collection,
        crate::handlers::collection::collection_summary,
        crate::handlers::collection::get_collection_entry,
        crate::handlers::collection::set_collection_entry,
        // --- Wish list ---
        crate::handlers::wishlist::list_wishlist,
        crate::handlers::wishlist::wishlist_summary,
        crate::handlers::wishlist::get_wishlist_entry,
        crate::handlers::wishlist::set_wishlist_entry,
        // --- API keys ---
        crate::handlers::api_keys::create_api_key,
        crate::handlers::api_keys::list_api_keys,
        crate::handlers::api_keys::revoke_api_key,
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
        (name = "Wish list", description = "The signed-in user's wanted-card holdings (requires an API key)."),
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
        assert_eq!(json["openapi"].as_str().unwrap_or_default().get(0..3), Some("3.1"));
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
    }
}
