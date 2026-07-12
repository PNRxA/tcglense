//! The embedded crawler HTML document. `render_document` emits a complete, valid,
//! escaped HTML5 page whose `<head>` reproduces exactly what `usePageMeta`
//! (`web/src/lib/seo.ts`) sets at runtime — in the same emission order — plus a
//! faithful, minimal human-readable `<body>` (an `<h1>`, the description, the preview
//! image, and the canonical link). The body is what makes this *dynamic rendering*
//! rather than cloaking: a crawler is served an equivalent representation of the page,
//! not a head-only shell.
//!
//! Self-contained: it never reads `dist/index.html`, so the renderer works when the
//! API serves no SPA (`WEB_ROOT` unset, the split-deploy `api` image).

use super::{PageMeta, SITE_NAME};

/// Build the crawler document for one resolved page.
pub(super) fn render_document(m: &PageMeta) -> String {
    // og:title / twitter:title / the visible <h1> use the raw title (no site suffix),
    // falling back to the site name — matching seo.ts's `title ?? SITE_NAME`. Only the
    // <title> element appends " · {SITE_NAME}".
    let display_title = if m.title.is_empty() { SITE_NAME } else { &m.title };
    let doc_title = if m.title.is_empty() {
        SITE_NAME.to_string()
    } else {
        format!("{} · {SITE_NAME}", m.title)
    };
    let robots = if m.noindex {
        "noindex, nofollow"
    } else {
        "index, follow"
    };

    let mut out = String::with_capacity(1024);
    out.push_str("<!doctype html><html lang=\"en\"><head>");
    out.push_str("<meta charset=\"utf-8\">");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">");
    push_el(&mut out, "<title>", &escape(&doc_title), "</title>");
    push_meta(&mut out, "name", "description", &m.description);
    push_meta(&mut out, "name", "robots", robots);
    if let Some(canonical) = &m.canonical {
        out.push_str("<link rel=\"canonical\" href=\"");
        out.push_str(&escape(canonical));
        out.push_str("\">");
    }
    push_meta(&mut out, "property", "og:site_name", SITE_NAME);
    push_meta(&mut out, "property", "og:type", m.og_type);
    push_meta(&mut out, "property", "og:title", display_title);
    push_meta(&mut out, "property", "og:description", &m.description);
    if let Some(canonical) = &m.canonical {
        push_meta(&mut out, "property", "og:url", canonical);
    }
    push_meta(&mut out, "property", "og:image", &m.image);
    push_meta(&mut out, "name", "twitter:card", "summary_large_image");
    push_meta(&mut out, "name", "twitter:title", display_title);
    push_meta(&mut out, "name", "twitter:description", &m.description);
    push_meta(&mut out, "name", "twitter:image", &m.image);
    if let Some(json_ld) = &m.json_ld {
        out.push_str("<script type=\"application/ld+json\">");
        out.push_str(&escape_json_ld(json_ld));
        out.push_str("</script>");
    }
    out.push_str("</head><body>");
    push_el(&mut out, "<h1>", &escape(display_title), "</h1>");
    push_el(&mut out, "<p>", &escape(&m.description), "</p>");
    out.push_str("<img src=\"");
    out.push_str(&escape(&m.image));
    out.push_str("\" alt=\"");
    out.push_str(&escape(display_title));
    out.push_str("\" width=\"1200\" height=\"630\">");
    if let Some(canonical) = &m.canonical {
        out.push_str("<p><a href=\"");
        out.push_str(&escape(canonical));
        out.push_str("\">");
        out.push_str(&escape(canonical));
        out.push_str("</a></p>");
    }
    out.push_str("</body></html>");
    out
}

fn push_el(out: &mut String, open: &str, escaped_inner: &str, close: &str) {
    out.push_str(open);
    out.push_str(escaped_inner);
    out.push_str(close);
}

/// Emit `<meta {attr}="{key}" content="{escaped value}">`.
fn push_meta(out: &mut String, attr: &str, key: &str, value: &str) {
    out.push_str("<meta ");
    out.push_str(attr);
    out.push_str("=\"");
    out.push_str(key);
    out.push_str("\" content=\"");
    out.push_str(&escape(value));
    out.push_str("\">");
}

/// Escape the five XML/HTML predefined entities (`& < > " '`), safe in double-quoted
/// attributes and in text. Same table as `sitemap.rs::xml_escape`.
pub(super) fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Serialize JSON-LD compact, then neutralise the `</script>` break-out (and stray
/// entities) the standard XSS-safe way: `<` `>` `&` → `<` `>` `&`.
/// These only ever occur inside JSON string values, so the result stays valid JSON.
pub(super) fn escape_json_ld(value: &serde_json::Value) -> String {
    serde_json::to_string(value)
        .unwrap_or_default()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_covers_the_five_entities() {
        assert_eq!(escape("a&b<c>d\"e'f"), "a&amp;b&lt;c&gt;d&quot;e&apos;f");
        assert_eq!(escape("plain"), "plain");
    }

    #[test]
    fn escape_json_ld_neutralises_script_breakout() {
        let v = serde_json::json!({ "name": "</script><x>&y" });
        let s = escape_json_ld(&v);
        assert!(!s.contains("</script>"));
        assert!(s.contains("\\u003c/script\\u003e"));
        assert!(s.contains("\\u0026y"));
    }

    #[test]
    fn document_title_uses_suffix_but_og_title_does_not() {
        let meta = PageMeta {
            title: "Black Lotus · Alpha".into(),
            description: "desc".into(),
            canonical: Some("https://x.test/cards/mtg/cards/1".into()),
            image: "https://x.test/og-image.png".into(),
            og_type: "product",
            noindex: false,
            json_ld: None,
        };
        let html = render_document(&meta);
        assert!(html.contains("<title>Black Lotus · Alpha · TCGLense</title>"));
        assert!(html.contains("<meta property=\"og:title\" content=\"Black Lotus · Alpha\">"));
        assert!(html.contains("<link rel=\"canonical\" href=\"https://x.test/cards/mtg/cards/1\">"));
        assert!(html.contains("index, follow"));
    }

    #[test]
    fn empty_title_falls_back_to_site_name() {
        let meta = PageMeta {
            title: String::new(),
            description: "home".into(),
            canonical: Some("https://x.test/".into()),
            image: "https://x.test/og-image.png".into(),
            og_type: "website",
            noindex: false,
            json_ld: None,
        };
        let html = render_document(&meta);
        assert!(html.contains("<title>TCGLense</title>"));
        assert!(html.contains("<meta property=\"og:title\" content=\"TCGLense\">"));
        assert!(html.contains("<h1>TCGLense</h1>"));
    }

    #[test]
    fn noindex_flag_flips_robots() {
        let meta = PageMeta {
            title: "Sign in".into(),
            description: "d".into(),
            canonical: None,
            image: "https://x.test/og-image.png".into(),
            og_type: "website",
            noindex: true,
            json_ld: None,
        };
        let html = render_document(&meta);
        assert!(html.contains("<meta name=\"robots\" content=\"noindex, nofollow\">"));
        // No canonical => no canonical link or og:url.
        assert!(!html.contains("rel=\"canonical\""));
        assert!(!html.contains("og:url"));
    }
}
