//! `robots.txt`, served by the API so its `Sitemap:` line is an absolute
//! `PUBLIC_SITE_URL` (issue #294 rationale). Drives the real router; the configured
//! origin under test is `test_state`'s `public_site_url` (`https://sitemap.test`).

use super::harness::*;

#[tokio::test]
async fn robots_txt_has_an_absolute_sitemap_and_disallows() {
    let app = test_app().await;
    let (status, headers, body) = send_text(&app, get("/robots.txt")).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type(&headers), Some("text/plain; charset=utf-8"));
    // The whole point of moving this to the API: an ABSOLUTE Sitemap URL, not a relative
    // one (the web build emitted `Sitemap: /sitemap.xml` when VITE_SITE_URL was unset).
    assert!(
        body.contains("Sitemap: https://sitemap.test/sitemap.xml"),
        "expected an absolute sitemap URL, got: {body}"
    );
    assert!(body.starts_with("User-agent: *"));
    assert!(body.contains("Allow: /"));
    // Auth + app + email-token routes stay disallowed.
    for path in ["/login", "/collection", "/wishlist", "/verify-email", "/reset-password"] {
        assert!(body.contains(&format!("Disallow: {path}")), "missing Disallow {path}: {body}");
    }
    // Shared-cacheable, like the sitemap.
    assert_eq!(
        cache_control(&headers),
        Some("public, max-age=3600, s-maxage=86400, stale-while-revalidate=604800")
    );
}
