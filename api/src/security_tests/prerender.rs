//! Server-side crawler prerendering (`handlers::prerender`). Drives the real router
//! in-process, with a `WEB_ROOT` set (the combined-image posture) so the SPA fallback
//! and its crawler branch are both live. Pins the contract issue #302's client-side
//! `usePageMeta` couldn't cover: a non-JS unfurler must get per-route `<head>` meta,
//! not the homepage's — while a real browser still gets the SPA shell untouched.

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use sea_orm::{ActiveModelTrait, DatabaseConnection, Set};

use super::harness::*;
use crate::entities::{card, card_set};
use crate::test_support::insert_product;
use crate::{build_router, config::Config, state::AppState};

const BASE: &str = "https://prerender.test";
const CHROME_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
    AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36";

const INDEX_HTML: &str = "<!doctype html><html><head><title>TCGLense SPA</title></head>\
    <body><div id=\"app\"></div></body></html>";

/// A throwaway web root (index.html + a hashed asset), for the combined-image fallback.
fn make_web_root(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("tcglense-prerender-{name}"));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("assets")).expect("create web root");
    fs::write(dir.join("index.html"), INDEX_HTML).expect("write index.html");
    fs::write(dir.join("assets/app-abc123.js"), "console.log('spa')").expect("write asset");
    dir
}

/// Seed one set, one card *with an image*, and one product *with an image*, so the
/// resolvers exercise the image-proxy + JSON-LD branches with known field values.
async fn seed_fixtures(db: &DatabaseConnection) {
    let now = Utc::now();
    card_set::ActiveModel {
        game: Set("mtg".into()),
        code: Set("tst".into()),
        name: Set("Test Set".into()),
        card_count: Set(0),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert set");

    card::ActiveModel {
        game: Set("mtg".into()),
        external_id: Set("img-card".into()),
        name: Set("Imaged Card".into()),
        set_code: Set("tst".into()),
        set_name: Set("Test Set".into()),
        collector_number: Set("42".into()),
        rarity: Set(Some("rare".into())),
        type_line: Set(Some("Creature — Elf".into())),
        image_large: Set(Some("https://cards.example/large.jpg".into())),
        price_usd: Set(Some("12.50".into())),
        released_at: Set(Some("2024-01-02".into())),
        lang: Set("en".into()),
        digital: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    }
    .insert(db)
    .await
    .expect("insert card");

    // image_url is set by insert_product, so has_image resolves true.
    insert_product(db, "prod-1", "Booster Box", "tst", "play_display", Some("99.99")).await;
}

/// The combined-image app (WEB_ROOT set, fixtures seeded, distinctive origin).
async fn prerender_app(web_root: Option<PathBuf>) -> Router {
    let db = crate::test_support::migrated_memory_db().await;
    seed_fixtures(&db).await;
    let config = Config {
        web_root,
        public_site_url: BASE.to_string(),
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    build_router(state)
}

fn get_ua(uri: &str, ua: &str) -> Request<Body> {
    Request::builder()
        .method("GET")
        .uri(uri)
        .header("user-agent", ua)
        .body(Body::empty())
        .unwrap()
}

/// Extract the `application/ld+json` script content (for the no-offers invariant).
fn json_ld_of(html: &str) -> String {
    let open = "<script type=\"application/ld+json\">";
    let start = html.find(open).map(|i| i + open.len()).expect("json-ld script present");
    let end = html[start..].find("</script>").map(|i| i + start).expect("json-ld closes");
    html[start..end].to_string()
}

fn vary(headers: &HeaderMap) -> Option<&str> {
    headers.get("vary").and_then(|v| v.to_str().ok())
}

#[tokio::test]
async fn discordbot_gets_per_route_card_meta_not_the_homepage() {
    let app = prerender_app(Some(make_web_root("card"))).await;
    let (status, headers, body) =
        send_text(&app, get_ua("/cards/mtg/cards/img-card", "Discordbot/2.0")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/html; charset=utf-8"));
    // The whole point: the card's own title/canonical, not the homepage's.
    assert!(body.contains("<title>Imaged Card · Test Set · TCGLense</title>"), "{body}");
    assert!(body.contains("<meta property=\"og:title\" content=\"Imaged Card · Test Set\">"));
    assert!(body.contains("<link rel=\"canonical\" href=\"https://prerender.test/cards/mtg/cards/img-card\">"));
    assert!(body.contains("<meta property=\"og:url\" content=\"https://prerender.test/cards/mtg/cards/img-card\">"));
    // The image is the absolute proxy URL (not window.origin), on both og + twitter.
    let img = "https://prerender.test/api/games/mtg/cards/img-card/image?size=large";
    assert!(body.contains(&format!("<meta property=\"og:image\" content=\"{img}\">")), "{body}");
    assert!(body.contains(&format!("<meta name=\"twitter:image\" content=\"{img}\">")));
    assert!(body.contains("<meta property=\"og:type\" content=\"product\">"));
    assert!(body.contains("<meta name=\"robots\" content=\"index, follow\">"));
    assert!(body.contains("<meta name=\"twitter:card\" content=\"summary_large_image\">"));
    // Rich, keyword-y description carrying rarity + latest price.
    assert!(body.contains("Imaged Card") && body.contains("Rare") && body.contains("$12.50"), "{body}");
    // JSON-LD: a Product + a BreadcrumbList, and NO storefront claims.
    let ld = json_ld_of(&body);
    assert!(ld.contains("\"@type\":\"Product\""), "{ld}");
    assert!(ld.contains("\"@type\":\"BreadcrumbList\""), "{ld}");
    for banned in ["offers", "\"price\"", "availability", "AggregateOffer"] {
        assert!(!ld.contains(banned), "JSON-LD must not claim a storefront offer: {ld}");
    }
    // Bot HTML must not be shared-cached and must Vary on UA.
    assert_eq!(cache_control(&headers), Some("private, no-store"));
    assert_eq!(vary(&headers), Some("User-Agent"));

    // And it genuinely differs from the homepage the same bot gets.
    let (_s, _h, home) = send_text(&app, get_ua("/", "Discordbot/2.0")).await;
    assert!(home.contains("<meta property=\"og:title\" content=\"TCGLense\">"), "{home}");
}

#[tokio::test]
async fn set_and_product_pages_prerender() {
    let app = prerender_app(Some(make_web_root("set-product"))).await;

    let (status, _h, set) =
        send_text(&app, get_ua("/cards/mtg/sets/tst", "Slackbot-LinkExpanding 1.0")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(set.contains("<meta property=\"og:title\" content=\"Test Set\">"), "{set}");
    assert!(set.contains(
        "content=\"Browse cards from Test Set on TCGLense, with singles prices tracked over time.\""
    ));
    // A set page carries the default banner and no JSON-LD.
    assert!(set.contains("<meta property=\"og:image\" content=\"https://prerender.test/og-image.png\">"));
    assert!(!set.contains("application/ld+json"));

    let (status, _h, prod) = send_text(&app, get_ua("/sealed/mtg/prod-1", "Discordbot/2.0")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(prod.contains("<meta property=\"og:title\" content=\"Booster Box\">"), "{prod}");
    assert!(prod.contains("<link rel=\"canonical\" href=\"https://prerender.test/sealed/mtg/prod-1\">"));
    assert!(prod.contains(
        "<meta property=\"og:image\" content=\"https://prerender.test/api/games/mtg/products/prod-1/image?size=normal\">"
    ));
    assert!(prod.contains("<meta property=\"og:type\" content=\"product\">"));
    assert!(json_ld_of(&prod).contains("\"@type\":\"Product\""));
}

#[tokio::test]
async fn prerender_all_user_agents_drops_the_ua_gate() {
    let db = crate::test_support::migrated_memory_db().await;
    seed_fixtures(&db).await;
    let config = Config {
        web_root: Some(make_web_root("all-uas")),
        public_site_url: BASE.to_string(),
        prerender_all_user_agents: true,
        ..crate::test_support::test_config()
    };
    let http = reqwest::Client::builder().build().expect("http client");
    let image_http = reqwest::Client::builder().build().expect("image client");
    let state = AppState::new(config, db, http, image_http, None).expect("assemble app state");
    let app = build_router(state);

    // With the gate dropped, even a plain browser UA gets the prerender document.
    let (status, _h, body) = send_text(&app, get_ua("/cards/mtg/cards/img-card", CHROME_UA)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("<meta property=\"og:title\" content=\"Imaged Card · Test Set\">"),
        "{body}"
    );
    assert!(!body.contains("id=\"app\""), "the gate is dropped, so a browser gets the prerender, not the SPA");
    // Assets are still served statically (the extension gate is independent of the UA gate).
    let (status, _h, js) = send_text(&app, get_ua("/assets/app-abc123.js", CHROME_UA)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(js.contains("console.log('spa')"));
}

#[tokio::test]
async fn a_browser_ua_still_gets_the_spa_shell() {
    let app = prerender_app(Some(make_web_root("browser"))).await;

    // Explicit browser UA.
    let (status, headers, body) = send_text(&app, get_ua("/cards/mtg/cards/img-card", CHROME_UA)).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("id=\"app\""), "browser must get index.html, got: {body}");
    assert!(!body.contains("og:title"), "browser must not get the prerender doc");
    assert_eq!(cache_control(&headers), Some("public, no-cache"));
    assert_eq!(vary(&headers), Some("User-Agent"));

    // No UA at all → also the SPA shell.
    let (_s, _h, body) = send_text(&app, get("/cards/mtg/cards/img-card")).await;
    assert!(body.contains("id=\"app\""));
}

#[tokio::test]
async fn noindex_app_route_gets_generic_noindex_meta() {
    let app = prerender_app(Some(make_web_root("noindex"))).await;
    let (status, _h, body) = send_text(&app, get_ua("/login", "Twitterbot/1.0")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<title>Sign in · TCGLense</title>"), "{body}");
    assert!(body.contains("<meta name=\"robots\" content=\"noindex, nofollow\">"));
}

#[tokio::test]
async fn an_unknown_card_id_is_a_404_soft_404() {
    let app = prerender_app(Some(make_web_root("notfound"))).await;
    let (status, _h, body) =
        send_text(&app, get_ua("/cards/mtg/cards/deadbeef-0000", "facebookexternalhit/1.1")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.contains("<title>Page not found · TCGLense</title>"), "{body}");
    assert!(body.contains("<meta name=\"robots\" content=\"noindex, nofollow\">"));
    assert!(!body.contains("rel=\"canonical\""));
}

#[tokio::test]
async fn assets_and_unknown_api_paths_are_never_prerendered() {
    let app = prerender_app(Some(make_web_root("assets"))).await;

    // A crawler fetching a hashed asset gets the file, not HTML — even when it sends
    // `Accept: text/html` (the extension gate must be authoritative; a dotted path like a
    // hashed asset or /robots.txt must never be intercepted as a page).
    let asset_req = Request::builder()
        .method("GET")
        .uri("/assets/app-abc123.js")
        .header("user-agent", "Discordbot/2.0")
        .header("accept", "text/html,application/xhtml+xml,*/*")
        .body(Body::empty())
        .unwrap();
    let (status, headers, body) = send_text(&app, asset_req).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("console.log('spa')"), "{body}");
    assert_eq!(cache_control(&headers), Some("public, max-age=31536000, immutable"));

    // An unknown /api path stays a JSON 404 (the catch-all wins over the fallback).
    let (status, _h, body) = send(&app, get_ua("/api/definitely-not-a-route", "Discordbot/2.0")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(body.get("error").is_some(), "expected JSON error, got: {body}");
}

#[tokio::test]
async fn explicit_route_renders_without_a_web_root() {
    // The split (`api`-only) deploy: no WEB_ROOT, Caddy proxies crawler HTML to
    // /api/prerender/*. The route is not UA-gated, so no UA is needed here.
    let app = prerender_app(None).await;
    let (status, headers, body) =
        send_text(&app, get("/api/prerender/cards/mtg/cards/img-card")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("<meta property=\"og:title\" content=\"Imaged Card · Test Set\">"), "{body}");
    assert!(body.contains(
        "<meta property=\"og:image\" content=\"https://prerender.test/api/games/mtg/cards/img-card/image?size=large\">"
    ));
    assert_eq!(cache_control(&headers), Some("private, no-store"));
    assert_eq!(vary(&headers), Some("User-Agent"));

    // The bare prefix renders the homepage: the split-deploy Caddy rewrites `/` to
    // `/api/prerender/` (empty tail), which the {*path} catch-all won't match, so it needs
    // its own route — else the most-shared URL would 404 on the api-only image.
    for uri in ["/api/prerender/", "/api/prerender"] {
        let (status, _h, body) = send_text(&app, get(uri)).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert!(body.contains("<title>TCGLense</title>"), "{uri}: {body}");
        assert!(body.contains("<meta property=\"og:title\" content=\"TCGLense\">"), "{uri}: {body}");
    }

    // Without WEB_ROOT there's no SPA fallback, so a normal browser path is still a 404.
    let (status, _h, _b) = send_text(&app, get("/cards/mtg/cards/img-card")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}
