//! Type-based filing for otherwise uncategorised deck-import rows.
//!
//! Provider categories remain authoritative. Only generic mainboard rows reach this
//! classifier, which mirrors the SPA's automatic target for a manually added card.

/// Whether a provider section carries no more information than "this is in the deck".
pub(super) fn is_generic_section(section: &str) -> bool {
    matches!(
        section.trim().to_ascii_lowercase().as_str(),
        "deck" | "main" | "mainboard"
    )
}

/// Pick the first matching preset type bucket from the card's front-face type line.
/// Multi-type permanents use the most deck-building-specific bucket (for example an
/// Artifact Creature files under Creatures), and modal back faces do not turn a spell
/// into a land for filing purposes.
pub(super) fn preset_section(type_line: Option<&str>) -> Option<&'static str> {
    let front = type_line?.split("//").next().unwrap_or_default();
    let has_type = |wanted: &str| {
        front
            .split(|character: char| !character.is_alphabetic())
            .any(|word| word.eq_ignore_ascii_case(wanted))
    };

    if has_type("Land") {
        Some("Lands")
    } else if has_type("Creature") {
        Some("Creatures")
    } else if has_type("Planeswalker") {
        Some("Planeswalkers")
    } else if has_type("Instant") {
        Some("Instants")
    } else if has_type("Sorcery") {
        Some("Sorceries")
    } else if has_type("Enchantment") {
        Some("Enchantments")
    } else if has_type("Artifact") {
        Some("Artifacts")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_generic_sections_case_insensitively() {
        assert!(is_generic_section(" MainBoard "));
        assert!(is_generic_section("deck"));
        assert!(!is_generic_section("Sideboard"));
        assert!(!is_generic_section("Ramp"));
    }

    #[test]
    fn files_multitype_and_modal_cards_by_the_front_face() {
        assert_eq!(preset_section(Some("Basic Land — Island")), Some("Lands"));
        assert_eq!(
            preset_section(Some("Artifact Creature — Golem")),
            Some("Creatures")
        );
        assert_eq!(preset_section(Some("Planeswalker")), Some("Planeswalkers"));
        assert_eq!(preset_section(Some("Instant")), Some("Instants"));
        assert_eq!(preset_section(Some("Sorcery // Land")), Some("Sorceries"));
        assert_eq!(preset_section(Some("Enchantment")), Some("Enchantments"));
        assert_eq!(preset_section(Some("Artifact")), Some("Artifacts"));
        assert_eq!(preset_section(Some("Battle — Siege")), None);
        assert_eq!(preset_section(None), None);
    }
}
