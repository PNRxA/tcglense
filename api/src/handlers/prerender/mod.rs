//! Server-side "dynamic rendering for bots": crawler User-Agents that don't run
//! JavaScript (social/link unfurlers — Discord, Slack, Facebook, X, …) get a small,
//! complete HTML document whose `<head>` reproduces exactly what `usePageMeta`
//! (`web/src/lib/seo.ts`) sets per route at runtime, plus a faithful human-readable
//! `<body>`. Ordinary browsers are untouched: they still receive `index.html` and
//! render the SPA.
//!
//! Without this, every non-JS crawler fetching any URL sees `index.html`'s static
//! homepage tags — so every card/set/product link unfurls as the generic homepage.
//!
//! ## Shape
//! * [`classify_path`] maps a request path to a [`RouteKind`], mirroring
//!   `web/src/router/index.ts`.
//! * [`resolve_meta`] fills a [`PageMeta`] per route, reusing the **same** SeaORM
//!   lookups the catalog handlers use ([`load_card`]/[`load_set`]/[`product_response`])
//!   and building every absolute URL from `config.public_site_url` — never a client
//!   origin. At most **one** indexed DB read and **zero** HTTP/image fetches per hit
//!   (the `og:image` is only ever a URL string; the unfurler fetches it later via the
//!   unchanged, host-allow-listed image proxy).
//! * [`template::render_document`] emits the escaped HTML from a `PageMeta`.
//!
//! ## Wiring (see `router.rs`)
//! * Combined image (`WEB_ROOT` set): [`prerender_fallback`] layers over the SPA
//!   fallback and branches on the UA before `index.html` is served.
//! * Split image (`api` only): the always-on `GET /api/prerender/{*path}` route
//!   ([`prerender_route`]) that Caddy reverse-proxies crawler HTML requests to. The
//!   renderer never reads `dist`, so it works with `WEB_ROOT` unset.
//!
//! Bot HTML is `private, no-store` + `Vary: User-Agent`, and the SPA shell also carries
//! `Vary: User-Agent`, so a shared CDN can never hand a bot's HTML to a browser.

mod structured_data;
mod template;

use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::error::AppError;
use crate::extract::Path;
use crate::handlers::catalog::product_response;
use crate::handlers::shared::{CardResponse, load_card, load_set};
use crate::state::AppState;

use structured_data as sd;

/// Product/brand name (TS twin: `seo.ts::SITE_NAME`). The `<title>` suffix + `og:site_name`.
const SITE_NAME: &str = "TCGLense";

/// Fallback description for pages that set none — noindex app pages + the 404
/// (TS twin: `seo.ts::SITE_DESCRIPTION`).
const SITE_DESCRIPTION: &str = "Track trading-card prices over time, catalogue your \
    collection and wish list, and use ghost mode to see exactly which cards you're missing.";

/// The site-wide default social banner path (TS twin: `seo.ts::DEFAULT_OG_IMAGE`).
const DEFAULT_OG_IMAGE_PATH: &str = "/og-image.png";

/// HomeView sets its own description (differs from `SITE_DESCRIPTION`); mirror it verbatim.
const HOME_DESCRIPTION: &str = "Browse trading-card games, sets, cards, and sealed products, \
    chart daily prices, and track your collection and wish list — with ghost mode showing \
    exactly which cards you are missing.";

// ---------- Route classification ----------

/// One SPA route, mirroring `web/src/router/index.ts`. Owned `String`s so the resolver
/// can query without borrowing the request path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RouteKind {
    // Indexable, fully static meta (no DB, no registry):
    Home,
    CardsHub,
    SealedHub,
    Docs,
    Terms,
    Privacy,
    // Indexable, GAMES registry only (no DB) — game-name resolution:
    GameHub(String),
    CardsBrowse(String),
    SealedGame(String),
    // Indexable, exactly one DB read:
    CardDetail { game: String, id: String },
    SetDetail { game: String, code: String },
    ProductDetail { game: String, id: String },
    /// noindex app/auth/email page — 200, generic doc, no DB. Carries the SPA's static title.
    Noindex(String),
    /// Unrouted path — a 404 soft-404 (mirrors `NotFoundView`).
    NotFound,
}

/// Classify a request path (query already excluded by `uri().path()`), mirroring
/// vue-router's first-match-wins order and exact segment counts. A single trailing
/// slash is dropped (except root). Segments are matched raw — MTG ids/codes/game slugs
/// carry no path-significant characters.
pub(crate) fn classify_path(path: &str) -> RouteKind {
    let trimmed = path.trim_end_matches('/');
    let segs: Vec<&str> = if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.trim_start_matches('/').split('/').collect()
    };
    let s = |x: &str| x.to_string();
    match segs.as_slice() {
        [] => RouteKind::Home,
        ["cards"] => RouteKind::CardsHub,
        ["cards", g] => RouteKind::GameHub(s(g)),
        ["cards", g, "cards"] => RouteKind::CardsBrowse(s(g)),
        ["cards", g, "cards", id] => RouteKind::CardDetail { game: s(g), id: s(id) },
        ["cards", g, "sets", code] => RouteKind::SetDetail { game: s(g), code: s(code) },
        ["sealed"] => RouteKind::SealedHub,
        ["sealed", g] => RouteKind::SealedGame(s(g)),
        ["sealed", g, id] => RouteKind::ProductDetail { game: s(g), id: s(id) },
        ["docs"] => RouteKind::Docs,
        ["terms"] => RouteKind::Terms,
        ["privacy"] => RouteKind::Privacy,
        // noindex app/auth/email routes — static titles from web/src/router + the views.
        ["login"] => RouteKind::Noindex(s("Sign in")),
        ["register"] => RouteKind::Noindex(s("Create your account")),
        ["complete-registration"] => RouteKind::Noindex(s("Finish creating your account")),
        ["forgot-password"] => RouteKind::Noindex(s("Forgot password")),
        ["reset-password"] => RouteKind::Noindex(s("Choose a new password")),
        ["verify-email"] => RouteKind::Noindex(s("Verify your email")),
        ["profile"] => RouteKind::Noindex(s("Your profile")),
        ["settings"] => RouteKind::Noindex(s("Settings")),
        ["scan"] => RouteKind::Noindex(s("Scan cards")),
        ["collection"] => RouteKind::Noindex(s("Your collections")),
        ["wishlist"] => RouteKind::Noindex(s("Your wish lists")),
        // Parameterised noindex. The game name is resolved via the static registry
        // (matching the SPA's useGameName); the set part keeps the uppercased code (the
        // SPA's own pre-data-load fallback — a set-name lookup isn't worth a DB read on a
        // noindex, auth-gated page).
        ["collection", g] => RouteKind::Noindex(format!("Your {} collection", game_name(g))),
        ["collection", g, "cards"] => RouteKind::Noindex(format!("All your {} cards", game_name(g))),
        ["collection", g, "sets", code] => {
            RouteKind::Noindex(format!("{} — your {} collection", code.to_uppercase(), game_name(g)))
        }
        ["wishlist", g] => RouteKind::Noindex(format!("Your {} wish list", game_name(g))),
        ["wishlist", g, "cards"] => RouteKind::Noindex(format!("Your {} wish list cards", game_name(g))),
        ["wishlist", g, "sets", code] => {
            RouteKind::Noindex(format!("{} — your {} wish list", code.to_uppercase(), game_name(g)))
        }
        _ => RouteKind::NotFound,
    }
}

// ---------- Resolved page metadata ----------

/// The resolved head+body inputs for one page — byte-faithful to what `usePageMeta`
/// emits. `title` is raw (no `· TCGLense` suffix; empty for Home); the template adds
/// the suffix to the `<title>` element only.
pub(crate) struct PageMeta {
    pub title: String,
    pub description: String,
    /// Absolute canonical URL; `None` only for the 404.
    pub canonical: Option<String>,
    /// Absolute preview image (a proxy URL or the default banner).
    pub image: String,
    pub og_type: &'static str,
    pub noindex: bool,
    pub json_ld: Option<serde_json::Value>,
}

/// The default social banner, absolute against `base`.
fn default_image(base: &str) -> String {
    format!("{base}{DEFAULT_OG_IMAGE_PATH}")
}

/// The game's display name (TS twin: `useGameName`), or the uppercased slug for an
/// unknown game — no DB, the registry is static.
fn game_name(game: &str) -> String {
    crate::catalog::find(game).map_or_else(|| game.to_uppercase(), |g| g.name.to_string())
}

/// The normalized path (a single trailing slash dropped; root stays `/`), for the
/// noindex canonical.
fn normalized_path(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() { "/" } else { trimmed }
}

/// A simple indexable page (`website`, default banner, no JSON-LD).
fn page(title: &str, description: String, canonical_path: &str, base: &str) -> PageMeta {
    PageMeta {
        title: title.to_string(),
        description,
        canonical: Some(absolute(base, canonical_path)),
        image: default_image(base),
        og_type: "website",
        noindex: false,
        json_ld: None,
    }
}

/// Absolute URL for a root-relative path against `base` (no trailing slash on `base`).
fn absolute(base: &str, path: &str) -> String {
    if path == "/" {
        format!("{base}/")
    } else {
        format!("{base}{path}")
    }
}

/// Fill a [`PageMeta`] for a classified route. `path` is the raw request path (used
/// only to reconstruct the noindex canonical). A missing entity surfaces as
/// `AppError::NotFound`; the caller turns it into the 404 document.
async fn resolve_meta(state: &AppState, path: &str, kind: RouteKind) -> Result<PageMeta, AppError> {
    let base = &state.config.public_site_url;
    let meta = match kind {
        RouteKind::Home => PageMeta {
            title: String::new(),
            description: HOME_DESCRIPTION.to_string(),
            canonical: Some(format!("{base}/")),
            image: default_image(base),
            og_type: "website",
            noindex: false,
            json_ld: None,
        },
        RouteKind::CardsHub => page(
            "Browse trading card games",
            "Browse the trading card games tracked on TCGLense and explore their sets, cards, and prices."
                .to_string(),
            "/cards",
            base,
        ),
        RouteKind::SealedHub => page(
            "Sealed products",
            "Browse sealed trading-card products — booster boxes, bundles and decks — with current \
             prices and price history on TCGLense."
                .to_string(),
            "/sealed",
            base,
        ),
        RouteKind::Docs => page(
            "API Reference",
            "Interactive reference for the TCGLense public API — anonymous catalog reads for cards, \
             sets, sealed products and prices, plus scoped API keys for your collection and wish list."
                .to_string(),
            "/docs",
            base,
        ),
        RouteKind::Terms => page(
            "Terms of Service",
            "The terms of service for TCGLense — a free, open-source trading-card price and \
             collection tracker."
                .to_string(),
            "/terms",
            base,
        ),
        RouteKind::Privacy => page(
            "Privacy Policy",
            "What data TCGLense collects, why, and what it never does with it.".to_string(),
            "/privacy",
            base,
        ),
        RouteKind::GameHub(game) => {
            let name = game_name(&game);
            page(
                &name,
                format!("Browse {name} sets and cards on TCGLense, with singles prices tracked over time."),
                &format!("/cards/{game}"),
                base,
            )
        }
        RouteKind::CardsBrowse(game) => {
            let name = game_name(&game);
            page(
                &format!("All {name} cards"),
                format!("Search and browse every {name} card tracked on TCGLense, with current prices."),
                &format!("/cards/{game}/cards"),
                base,
            )
        }
        RouteKind::SealedGame(game) => {
            let name = game_name(&game);
            page(
                &format!("{name} sealed products"),
                format!(
                    "Browse and filter sealed {name} products — booster boxes, bundles and decks — \
                     with current prices and price history on TCGLense."
                ),
                &format!("/sealed/{game}"),
                base,
            )
        }
        RouteKind::SetDetail { game, code } => {
            let set = load_set(state, &game, &code).await?;
            page(
                &set.name,
                format!("Browse cards from {} on TCGLense, with singles prices tracked over time.", set.name),
                &format!("/cards/{game}/sets/{code}"),
                base,
            )
        }
        RouteKind::CardDetail { game, id } => {
            let card = CardResponse::from(load_card(state, &game, &id).await?);
            let image = card
                .has_image
                .then(|| format!("{base}/api/games/{game}/cards/{}/image?size=large", card.id));
            let json_ld = sd::graph(vec![
                sd::card_product_node(&card, image.as_deref()),
                sd::breadcrumb_list(base, &sd::card_crumbs(&game, &card)),
            ]);
            PageMeta {
                title: format!("{} · {}", card.name, card.set_name),
                description: sd::card_meta_description(&card),
                canonical: Some(format!("{base}/cards/{game}/cards/{}", card.id)),
                image: image.unwrap_or_else(|| default_image(base)),
                og_type: "product",
                noindex: false,
                json_ld: Some(json_ld),
            }
        }
        RouteKind::ProductDetail { game, id } => {
            let product = product_response(state, &game, &id).await?;
            let type_label = sd::product_type_label(&product.product_type);
            let set_name = product
                .set_name
                .clone()
                .unwrap_or_else(|| product.set_code.to_uppercase());
            let image = product
                .has_image
                .then(|| format!("{base}/api/games/{game}/products/{}/image?size=normal", product.id));
            let json_ld = sd::graph(vec![
                sd::sealed_product_node(&product, &type_label, &set_name, image.as_deref()),
                sd::breadcrumb_list(base, &sd::sealed_crumbs(&game, &product)),
            ]);
            PageMeta {
                title: product.name.clone(),
                description: sd::product_meta_description(&product, &type_label, &set_name),
                canonical: Some(format!("{base}/sealed/{game}/{}", product.id)),
                image: image.unwrap_or_else(|| default_image(base)),
                og_type: "product",
                noindex: false,
                json_ld: Some(json_ld),
            }
        }
        RouteKind::Noindex(title) => PageMeta {
            title,
            description: SITE_DESCRIPTION.to_string(),
            canonical: Some(absolute(base, normalized_path(path))),
            image: default_image(base),
            og_type: "website",
            noindex: true,
            json_ld: None,
        },
        // Handled by the caller (render_for_path), never reached here.
        RouteKind::NotFound => not_found_meta(base),
    };
    Ok(meta)
}

/// The generic soft-404 head+body (mirrors `NotFoundView`: title "Page not found",
/// noindex, no canonical, default banner).
fn not_found_meta(base: &str) -> PageMeta {
    PageMeta {
        title: "Page not found".to_string(),
        description: SITE_DESCRIPTION.to_string(),
        canonical: None,
        image: default_image(base),
        og_type: "website",
        noindex: true,
        json_ld: None,
    }
}

/// Classify → resolve → (status, html). A missing entity becomes the 404 document; any
/// other error (e.g. the DB is down) propagates so the caller can fall back to the SPA
/// shell rather than emit a broken page.
async fn render_for_path(state: &AppState, path: &str) -> Result<(StatusCode, String), AppError> {
    let kind = classify_path(path);
    let base = &state.config.public_site_url;
    let (status, meta) = match kind {
        RouteKind::NotFound => (StatusCode::NOT_FOUND, not_found_meta(base)),
        other => match resolve_meta(state, path, other).await {
            Ok(meta) => (StatusCode::OK, meta),
            Err(AppError::NotFound(_)) => (StatusCode::NOT_FOUND, not_found_meta(base)),
            Err(err) => return Err(err),
        },
    };
    Ok((status, template::render_document(&meta)))
}

// ---------- Crawler / navigation detection ----------

/// Lowercased crawler UA needles: social/link unfurlers (never run JS — the ones broken
/// today) plus search bots that benefit from server-rendered meta. Substring match on
/// the lowercased UA. Keep in sync with the `@crawlerHtml` regex in the Caddyfiles.
const CRAWLER_UA_NEEDLES: &[&str] = &[
    // Social / link unfurlers — server-side fetchers with distinctive UAs. A few are brand
    // words that can also appear in an app's in-app WebView UA (pinterest, tumblr,
    // flipboard, mastodon, bluesky): a deliberate LOW-severity trade-off — such a
    // navigation gets the readable prerender doc (which links to the canonical) rather than
    // the SPA. Brands that collide with a real browser but have low unfurl value (snapchat,
    // viber, sogou) are omitted, and the precise bot token is used where one exists
    // (yandexbot, not the Yandex browser). Keep this in EXACT sync with the `@crawlerHtml`
    // regex in deploy/web.Caddyfile + deploy/Caddyfile.
    "facebookexternalhit",
    "facebot",
    "twitterbot",
    "slackbot",
    "slack-imgproxy",
    "discordbot",
    "linkedinbot",
    "whatsapp",
    "telegrambot",
    "redditbot",
    "pinterest",
    "applebot",
    "skypeuripreview",
    "embedly",
    "quora link preview",
    "nuzzel",
    "flipboard",
    "tumblr",
    "mastodon",
    "bluesky",
    "cardyb",
    "iframely",
    "google-inspectiontool",
    // Search engines (JS-limited or benefit from server-rendered meta):
    "googlebot",
    "bingbot",
    "duckduckbot",
    "yandexbot",
    "baiduspider",
];

/// Whether a User-Agent belongs to a known crawler.
pub(crate) fn is_crawler_ua(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    CRAWLER_UA_NEEDLES.iter().any(|needle| ua.contains(needle))
}

/// Whether this is an HTML navigation (a page), not an asset/file fetch: the path's last
/// segment has no file extension. Every SPA route is extensionless (MTG ids/codes/slugs
/// contain no `.`), so this is the authoritative signal — and it must NOT be overridden by
/// the `Accept` header, or a crawler fetching a dotted file with `Accept: text/html`
/// (some send it for `/robots.txt`) would be intercepted instead of served the file. Keeps
/// `/assets/*.js`, `/favicon.ico`, `/og-image.png`, `/robots.txt`, `/sitemap.xml` static.
fn is_html_navigation(req: &Request) -> bool {
    let last = req.uri().path().rsplit('/').next().unwrap_or("");
    !last.contains('.')
}

// ---------- HTTP responses + wiring ----------

/// Build the prerender HTTP response. `Vary: User-Agent` so a shared cache can't hand a
/// bot's HTML to a browser (or vice-versa); `no-store` so bot HTML is never stored (it
/// is one cheap DB read to regenerate). HEAD → headers only.
fn prerender_response(status: StatusCode, html: String, head: bool) -> Response {
    let body = if head { String::new() } else { html };
    (
        status,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8")),
            (header::CACHE_CONTROL, HeaderValue::from_static("private, no-store")),
            (header::VARY, HeaderValue::from_static("User-Agent")),
        ],
        body,
    )
        .into_response()
}

/// Combined-image SPA-fallback branch: a crawler UA on an HTML GET/HEAD gets the
/// prerendered document; everyone else (browsers, asset fetches, and prerender errors)
/// passes through unchanged to `ServeDir`/`index.html`. Never rewrites the request path.
pub async fn prerender_fallback(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let is_doc = matches!(request.method(), &Method::GET | &Method::HEAD);
    // Normally only crawler UAs are prerendered; PRERENDER_ALL_USER_AGENTS drops that gate
    // and prerenders every client on an HTML navigation (browsers then get the prerender
    // document instead of the SPA — see the config field docs).
    let target = state.config.prerender_all_user_agents
        || request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(is_crawler_ua);
    if is_doc && target && is_html_navigation(&request) {
        let head = *request.method() == Method::HEAD;
        let path = request.uri().path().to_owned();
        match render_for_path(&state, &path).await {
            Ok((status, html)) => return prerender_response(status, html, head),
            Err(err) => tracing::warn!(%path, error = %err, "prerender failed; serving SPA shell"),
        }
    }
    next.run(request).await
}

/// `GET /api/prerender/{*path}` — the always-on renderer split deploys proxy crawler
/// HTML requests to. `{*path}` is captured without a leading slash (matchit), so rebuild
/// the SPA path. Not UA-gated: Caddy only proxies crawlers here; a direct browser hit
/// just gets correct, public HTML (harmless). A DB/internal error becomes a normal
/// `AppError` response (5xx) so Caddy can fall through, never a broken document.
pub async fn prerender_route(State(state): State<AppState>, Path(path): Path<String>) -> Response {
    let spa_path = format!("/{path}");
    match render_for_path(&state, &spa_path).await {
        Ok((status, html)) => prerender_response(status, html, false),
        Err(err) => err.into_response(),
    }
}

/// The bare `/api/prerender` / `/api/prerender/` prefix → the Home document. matchit's
/// `{*path}` catch-all does **not** match an empty tail, so the homepage (`/`), which the
/// split-deploy Caddy rewrites to `/api/prerender/` (`uri` = `/`), needs its own route —
/// otherwise the most-shared URL 404s on the `api`-only image. See [`prerender_route`].
pub async fn prerender_root(State(state): State<AppState>) -> Response {
    match render_for_path(&state, "/").await {
        Ok((status, html)) => prerender_response(status, html, false),
        Err(err) => err.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_covers_every_route_kind() {
        assert_eq!(classify_path(""), RouteKind::Home);
        assert_eq!(classify_path("/"), RouteKind::Home);
        assert_eq!(classify_path("/cards"), RouteKind::CardsHub);
        assert_eq!(classify_path("/cards/mtg"), RouteKind::GameHub("mtg".into()));
        assert_eq!(classify_path("/cards/mtg/cards"), RouteKind::CardsBrowse("mtg".into()));
        assert_eq!(
            classify_path("/cards/mtg/cards/abc-123"),
            RouteKind::CardDetail { game: "mtg".into(), id: "abc-123".into() }
        );
        assert_eq!(
            classify_path("/cards/mtg/sets/blb"),
            RouteKind::SetDetail { game: "mtg".into(), code: "blb".into() }
        );
        assert_eq!(classify_path("/sealed"), RouteKind::SealedHub);
        assert_eq!(classify_path("/sealed/mtg"), RouteKind::SealedGame("mtg".into()));
        assert_eq!(
            classify_path("/sealed/mtg/42"),
            RouteKind::ProductDetail { game: "mtg".into(), id: "42".into() }
        );
        assert_eq!(classify_path("/docs"), RouteKind::Docs);
        assert_eq!(classify_path("/terms"), RouteKind::Terms);
        assert_eq!(classify_path("/privacy"), RouteKind::Privacy);
        // noindex
        assert!(matches!(classify_path("/login"), RouteKind::Noindex(_)));
        assert!(matches!(classify_path("/collection/mtg/sets/blb"), RouteKind::Noindex(_)));
        assert!(matches!(classify_path("/wishlist/mtg"), RouteKind::Noindex(_)));
        // Parameterised noindex resolves the game display name (not the raw slug).
        assert_eq!(
            classify_path("/collection/mtg"),
            RouteKind::Noindex("Your Magic: The Gathering collection".into())
        );
        // trailing slash equivalence
        assert_eq!(classify_path("/cards/mtg/"), classify_path("/cards/mtg"));
        // unrouted / too-deep
        assert_eq!(classify_path("/cards/mtg/cards/x/y"), RouteKind::NotFound);
        assert_eq!(classify_path("/nope"), RouteKind::NotFound);
    }

    #[test]
    fn crawler_ua_detection() {
        assert!(is_crawler_ua("Discordbot/2.0"));
        assert!(is_crawler_ua("facebookexternalhit/1.1"));
        assert!(is_crawler_ua("Slackbot-LinkExpanding 1.0"));
        assert!(is_crawler_ua("Mozilla/5.0 (compatible; Googlebot/2.1; +http://www.google.com/bot.html)"));
        assert!(!is_crawler_ua(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120"
        ));
        assert!(!is_crawler_ua(""));
    }

    #[test]
    fn game_name_falls_back_to_uppercase() {
        assert_eq!(game_name("mtg"), "Magic: The Gathering");
        assert_eq!(game_name("zzz"), "ZZZ");
    }

    #[test]
    fn html_navigation_gates_on_extension_only() {
        let req = |uri: &str| {
            Request::builder()
                .uri(uri)
                .body(axum::body::Body::empty())
                .unwrap()
        };
        // Pages (extensionless) — navigations.
        assert!(is_html_navigation(&req("/")));
        assert!(is_html_navigation(&req("/cards/mtg/cards/0000419b-0bba-4488")));
        assert!(is_html_navigation(&req("/cards/mtg/")));
        // Assets/files (dotted last segment) — never navigations, even for a bot.
        assert!(!is_html_navigation(&req("/robots.txt")));
        assert!(!is_html_navigation(&req("/og-image.png")));
        assert!(!is_html_navigation(&req("/assets/app-abc.js")));
        assert!(!is_html_navigation(&req("/sitemap.xml")));
    }
}
