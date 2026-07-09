//! DB-backed sitemaps (issues #75 and #294).
//!
//! The sitemap index + child sitemaps advertise the public catalog to crawlers.
//! These drive the real router so the route wiring, the XML shape, the configured
//! public-site-URL `<loc>`s, and the cache policy are all covered end to end. The
//! configured origin under test is `test_state`'s `public_site_url`
//! (`https://sitemap.test`).

use super::harness::*;
use crate::handlers::sitemap::SITEMAP_CACHE_CONTROL;

#[tokio::test]
async fn sitemap_index_lists_child_sitemaps() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/sitemap.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type(&headers)
            .unwrap()
            .starts_with("application/xml")
    );
    // A sitemap is expensive to build and changes at most daily, so it gets the
    // longer, shared-cacheable sitemap policy (not the catalog default).
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));

    assert!(body.contains("<sitemapindex"), "not an index doc: {body}");
    // Children are referenced against the configured public site origin, at the
    // root /sitemaps/ path (not /api/).
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/pages.xml</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/sets.xml</loc>"));
    // The seeded catalog has cards and sealed products, so there is at least one
    // chunk of each.
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/cards-1.xml</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/products-1.xml</loc>"));
    assert!(
        !body.contains("/api/sitemaps/"),
        "children must live at the root: {body}"
    );
}

#[tokio::test]
async fn sitemap_api_aliases_still_answer() {
    // The pre-#294 URLs are already submitted to search consoles and pointed at by
    // deployed robots.txt files, so the /api/ paths keep serving the same documents.
    let app = test_app_with_catalog().await;

    let (status, _h, body) = send_text(&app, get("/api/sitemap.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<sitemapindex"), "not an index doc: {body}");
    // Even via the alias, children are advertised at the root path.
    assert!(body.contains("<loc>https://sitemap.test/sitemaps/pages.xml</loc>"));

    let (status, _h, body) = send_text(&app, get("/api/sitemaps/pages.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<urlset"), "not a urlset doc: {body}");
}

#[tokio::test]
async fn sitemap_pages_covers_static_and_game_routes() {
    let app = test_app_with_catalog().await;
    let (status, headers, body) = send_text(&app, get("/sitemaps/pages.xml")).await;

    assert_eq!(status, StatusCode::OK);
    assert!(
        content_type(&headers)
            .unwrap()
            .starts_with("application/xml")
    );
    assert!(body.contains("<urlset"));
    assert!(body.contains("<loc>https://sitemap.test/</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/cards/mtg/cards</loc>"));
    // The sealed hub + per-game browse and the legal pages (issue #294).
    assert!(body.contains("<loc>https://sitemap.test/sealed</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/sealed/mtg</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/terms</loc>"));
    assert!(body.contains("<loc>https://sitemap.test/privacy</loc>"));
}

#[tokio::test]
async fn sitemap_sets_lists_seeded_sets() {
    let app = test_app_with_catalog().await;

    // Discover a real seeded set code from the catalog, then assert the sitemap
    // advertises its SPA detail page.
    let (_s, _h, sets) = send(&app, get("/api/games/mtg/sets")).await;
    let code = sets["data"][0]["code"].as_str().expect("a seeded set code");

    let (status, _h, body) = send_text(&app, get("/sitemaps/sets.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains(&format!(
            "<loc>https://sitemap.test/cards/mtg/sets/{code}</loc>"
        )),
        "set {code} missing from sitemap: {body}"
    );
}

#[tokio::test]
async fn sitemap_cards_chunk_lists_cards_and_out_of_range_is_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/sitemaps/cards-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));
    assert!(
        body.contains("<loc>https://sitemap.test/cards/mtg/cards/"),
        "no card URLs in chunk: {body}"
    );

    // A chunk past the end is a 404, and — like every error — is never shared-cached.
    let (status, headers, _b) = send_text(&app, get("/sitemaps/cards-9999.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));

    // An unknown child name is likewise a 404.
    let (status, _h, _b) = send_text(&app, get("/sitemaps/bogus.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn sitemap_products_chunk_lists_sealed_products_and_out_of_range_is_404() {
    let app = test_app_with_catalog().await;

    let (status, headers, body) = send_text(&app, get("/sitemaps/products-1.xml")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cache_control(&headers), Some(SITEMAP_CACHE_CONTROL));
    assert!(
        body.contains("<loc>https://sitemap.test/sealed/mtg/"),
        "no sealed-product URLs in chunk: {body}"
    );

    let (status, headers, _b) = send_text(&app, get("/sitemaps/products-9999.xml")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(cache_control(&headers), Some("no-store"));
}
