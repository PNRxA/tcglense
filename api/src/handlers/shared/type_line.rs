//! Reading a card's Scryfall type line: the supertype vocabulary, the
//! `(types, subtypes, supertypes)` split the Archidekt CSV export writes, and the
//! **primary type** the collection breakdown buckets by (issue #680). One seam so the
//! export's columns and the breakdown's buckets can't disagree on what counts as a
//! supertype.

/// MTG supertypes — the fixed leading words that Archidekt splits into its own column
/// and that the breakdown's primary type skips over.
pub(crate) const SUPERTYPES: &[&str] = &[
    "Basic",
    "Legendary",
    "Ongoing",
    "Snow",
    "World",
    "Host",
    "Elite",
];

/// The front face of a multi-faced type line (`"A — B // C — D"` → `"A — B"`), and its
/// left-of-the-dash half (supertypes + card types) and right half (subtypes).
fn front_face_halves(line: &str) -> (&str, &str) {
    let front = line.split("//").next().unwrap_or(line).trim();
    match front.split_once('—') {
        Some((left, right)) => (left.trim(), right.trim()),
        None => (front, ""),
    }
}

/// Split a Scryfall type line into Archidekt's `(Types, Sub-types, Super-types)` columns
/// (each comma-joined). The left of the em dash holds supertypes + card types; the right
/// holds subtypes. For a multi-faced card (`"A — B // C — D"`) only the front face is
/// used, matching Archidekt. A type line without an em dash has no subtypes.
pub(crate) fn split_type_line(type_line: Option<&str>) -> (String, String, String) {
    let Some(line) = type_line else {
        return (String::new(), String::new(), String::new());
    };
    let (left, right) = front_face_halves(line);

    let mut supertypes = Vec::new();
    let mut types = Vec::new();
    for word in left.split_whitespace() {
        if is_supertype(word) {
            supertypes.push(word);
        } else {
            types.push(word);
        }
    }
    let subtypes: Vec<&str> = right.split_whitespace().collect();

    (types.join(","), subtypes.join(","), supertypes.join(","))
}

fn is_supertype(word: &str) -> bool {
    SUPERTYPES.iter().any(|s| s.eq_ignore_ascii_case(word))
}

/// The **first card type** on a type line, as printed (`"Creature"`), skipping supertypes
/// and reading only the front face — the bucket the collection breakdown files a card
/// under. Scryfall lists an artifact creature as `Artifact Creature`, so it files under
/// `Artifact`: the first word is the type the line *leads* with, and the breakdown's
/// contract is that one word rather than a per-type re-count. `None` for a missing or
/// supertype-only line (nothing to name).
pub(crate) fn primary_type(type_line: Option<&str>) -> Option<&str> {
    let (left, _) = front_face_halves(type_line?);
    left.split_whitespace().find(|word| !is_supertype(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_line_splits_into_types_subtypes_supertypes() {
        assert_eq!(
            split_type_line(Some("Legendary Creature — Human Avatar Ally")),
            (
                "Creature".into(),
                "Human,Avatar,Ally".into(),
                "Legendary".into()
            )
        );
        assert_eq!(
            split_type_line(Some("Basic Land — Forest")),
            ("Land".into(), "Forest".into(), "Basic".into())
        );
        // No em dash -> no subtypes.
        assert_eq!(
            split_type_line(Some("Enchantment")),
            ("Enchantment".into(), String::new(), String::new())
        );
        // Multi-faced: only the front face is used.
        assert_eq!(
            split_type_line(Some("Creature — Human // Creature — Spirit")),
            ("Creature".into(), "Human".into(), String::new())
        );
        assert_eq!(
            split_type_line(None),
            (String::new(), String::new(), String::new())
        );
    }

    #[test]
    fn primary_type_is_the_first_card_type_past_the_supertypes() {
        assert_eq!(
            primary_type(Some("Legendary Creature — Human Avatar Ally")),
            Some("Creature")
        );
        assert_eq!(primary_type(Some("Basic Land — Forest")), Some("Land"));
        // The line's leading type, not a per-type re-count: an artifact creature files
        // under Artifact, as Scryfall spells it.
        assert_eq!(
            primary_type(Some("Artifact Creature — Golem")),
            Some("Artifact")
        );
        // Multi-faced: the front face only.
        assert_eq!(primary_type(Some("Instant // Sorcery")), Some("Instant"));
        // A supertype-only or absent line has nothing to name.
        assert_eq!(primary_type(Some("Legendary")), None);
        assert_eq!(primary_type(Some("")), None);
        assert_eq!(primary_type(None), None);
    }
}
