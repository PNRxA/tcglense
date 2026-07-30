//! Small request-field validators shared by the authenticated container surfaces.
//!
//! Both `handlers::decks` and `handlers::tools::life` accept user-named rows (a deck, a
//! folder, a section, a tracked game, a seat), and every one of them wants the same two
//! rules: a required name is trimmed, must be non-empty, and is length-bounded; an optional
//! text field is trimmed, collapses to `None` when blank, and is length-bounded. They lived
//! in `handlers::decks` first — this is the extraction, so the second surface reuses the
//! seam instead of forking a second copy.

use crate::error::AppError;

/// Trim + validate a required name field (non-empty, at most `max` characters).
pub(crate) fn validate_name(value: &str, field: &str, max: usize) -> Result<String, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(format!("{field} must not be empty")));
    }
    if trimmed.chars().count() > max {
        return Err(AppError::Validation(format!(
            "{field} must be at most {max} characters"
        )));
    }
    Ok(trimmed.to_string())
}

/// Trim + validate an optional text field: blank collapses to `None`; over `max`
/// characters is a 422.
pub(crate) fn validate_optional(
    value: Option<String>,
    field: &str,
    max: usize,
) -> Result<Option<String>, AppError> {
    match value {
        Some(v) => {
            let trimmed = v.trim();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.chars().count() > max {
                return Err(AppError::Validation(format!(
                    "{field} must be at most {max} characters"
                )));
            }
            Ok(Some(trimmed.to_string()))
        }
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn required_name_is_trimmed_bounded_and_never_blank() {
        assert_eq!(validate_name("  Krenko  ", "name", 10).unwrap(), "Krenko");
        // A name that is only whitespace is blank, not a 200-with-spaces.
        assert!(validate_name("   ", "name", 10).is_err());
        // The bound counts characters, not bytes, so multi-byte names aren't
        // rejected early.
        assert_eq!(
            validate_name("日本語です", "name", 5).unwrap(),
            "日本語です"
        );
        assert!(validate_name("日本語ですよ", "name", 5).is_err());
    }

    #[test]
    fn optional_text_collapses_blank_to_none() {
        assert_eq!(validate_optional(None, "format", 10).unwrap(), None);
        assert_eq!(
            validate_optional(Some("  ".into()), "format", 10).unwrap(),
            None
        );
        assert_eq!(
            validate_optional(Some(" edh ".into()), "format", 10).unwrap(),
            Some("edh".to_string())
        );
        assert!(validate_optional(Some("x".repeat(11)), "format", 10).is_err());
    }
}
