//! Supported UI accent colours — the account's brand-hue preference.
//!
//! The SPA's design system paints its primary/ring tokens from a small set of
//! AA-validated presets (see `docs/design-system.md`); the chosen slug lives on the
//! account so the accent follows a user across devices. The server stores and validates
//! the *slug only* — the actual colour values are the SPA's (mirrored in
//! `web/src/lib/accent.ts`, with tests pinning both sides, like the life-counter layout
//! vocabulary). Free-form colours are deliberately not accepted: every preset ships with
//! WCAG-checked light/dark pairs, which an arbitrary hex could not guarantee.

use crate::error::AppError;

pub const DEFAULT_ACCENT: &str = "pink";

/// Keep in step with `ACCENT_OPTIONS` in `web/src/lib/accent.ts` — a slug added on one
/// side only is either rejected by the API or renders as the default.
pub const SUPPORTED_ACCENTS: &[&str] = &["pink", "ember", "violet", "teal", "blue", "green"];

pub fn is_supported(slug: &str) -> bool {
    SUPPORTED_ACCENTS.contains(&slug)
}

pub fn validate(slug: &str) -> Result<&str, AppError> {
    let slug = slug.trim();
    if is_supported(slug) {
        Ok(slug)
    } else {
        Err(AppError::Validation(format!(
            "accent must be one of {}",
            SUPPORTED_ACCENTS.join(", ")
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_accent_slugs_exactly() {
        // Pins the wire vocabulary the SPA mirrors in web/src/lib/accent.ts.
        assert_eq!(
            SUPPORTED_ACCENTS,
            &["pink", "ember", "violet", "teal", "blue", "green"]
        );
        assert!(SUPPORTED_ACCENTS.contains(&DEFAULT_ACCENT));
        assert_eq!(validate(" teal ").unwrap(), "teal");
        // Exact, case-sensitive membership — no normalisation beyond trimming.
        assert!(validate("Teal").is_err());
        assert!(validate("#ff00aa").is_err());
        assert!(validate("").is_err());
    }
}
