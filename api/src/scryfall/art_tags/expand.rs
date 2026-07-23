//! Pure hierarchy expansion for the art-tag ingest.
//!
//! The bulk file holds only *direct* taggings: a parent tag such as `animal` carries no
//! taggings of its own — its artworks are the union of its descendants'. Scryfall's own
//! `art:` search resolves that hierarchy, so for parity we expand it here, at ingest,
//! and store one `(tag, artwork)` row per tag **and every ancestor** — the search then
//! stays a single indexed `EXISTS` probe with no query-time tree walk.
//!
//! Everything in this module is pure and synchronous; the orchestration (download,
//! parse, DB swap) lives in the parent module.

use std::collections::{HashMap, HashSet};

/// One parsed tag from the bulk file, reduced to what expansion needs.
pub(super) struct TagInput {
    /// The tag's stable Tagger UUID.
    pub scryfall_id: String,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    /// Child tags (bulk-file UUIDs); unknown ids are ignored.
    pub child_ids: Vec<String>,
    /// Directly tagged artworks (`illustration_id`s).
    pub taggings: Vec<String>,
}

/// A tag after expansion: its metadata plus every known artwork in its subtree.
pub(super) struct ExpandedTag {
    pub scryfall_id: String,
    pub slug: String,
    pub label: String,
    pub description: Option<String>,
    /// Indices into [`Expanded::illustrations`], sorted and deduplicated.
    pub illustrations: Vec<u32>,
}

/// The expansion result: an interner of artwork ids plus the per-tag artwork sets.
pub(super) struct Expanded {
    /// Interned `illustration_id`s; [`ExpandedTag::illustrations`] indexes into this.
    /// Interning keeps the peak footprint small — each 36-byte UUID string is held
    /// once, not once per (tag, artwork) row.
    pub illustrations: Vec<String>,
    /// Tags whose expanded artwork set is non-empty, in input order. Tags that match
    /// nothing we store (digital-only artworks, empty branches) are dropped so the
    /// autocomplete never suggests a tag with zero results.
    pub tags: Vec<ExpandedTag>,
    /// Total mapping rows (`Σ tags[i].illustrations.len()`).
    pub rows: usize,
}

/// Expand the tag hierarchy over the artworks we actually store.
///
/// `known` is the set of `cards.illustration_id`s in the catalog; taggings outside it
/// are discarded (same scoping as the rulings import). Each tag's artwork set is its
/// direct taggings unioned with every descendant's, memoized so shared subtrees
/// (multi-parent tags are common) are computed once. A hierarchy cycle — not promised
/// impossible by the data — is broken by treating the back-edge as empty rather than
/// recursing forever.
pub(super) fn expand(tags: Vec<TagInput>, known: &HashSet<String>) -> Expanded {
    let index_of: HashMap<&str, usize> = tags
        .iter()
        .enumerate()
        .map(|(i, t)| (t.scryfall_id.as_str(), i))
        .collect();

    // Intern the known illustration ids and resolve each tag's direct taggings to
    // interned indices up front.
    let mut interner: HashMap<String, u32> = HashMap::new();
    let mut illustrations: Vec<String> = Vec::new();
    let direct: Vec<Vec<u32>> = tags
        .iter()
        .map(|t| {
            t.taggings
                .iter()
                .filter(|ill| known.contains(*ill))
                .map(|ill| {
                    *interner.entry(ill.clone()).or_insert_with(|| {
                        illustrations.push(ill.clone());
                        (illustrations.len() - 1) as u32
                    })
                })
                .collect()
        })
        .collect();

    let children: Vec<Vec<usize>> = tags
        .iter()
        .map(|t| {
            t.child_ids
                .iter()
                .filter_map(|id| index_of.get(id.as_str()).copied())
                .collect()
        })
        .collect();

    let mut state = Dfs {
        direct: &direct,
        children: &children,
        memo: vec![None; tags.len()],
        color: vec![Color::White; tags.len()],
    };
    for i in 0..tags.len() {
        state.subtree(i);
    }
    let memo = state.memo;

    let mut rows = 0usize;
    let expanded = tags
        .into_iter()
        .zip(memo)
        .filter_map(|(t, ills)| {
            let ills = ills.unwrap_or_default();
            if ills.is_empty() {
                return None;
            }
            rows += ills.len();
            Some(ExpandedTag {
                scryfall_id: t.scryfall_id,
                slug: t.slug,
                label: t.label,
                description: t.description,
                illustrations: ills,
            })
        })
        .collect();

    Expanded {
        illustrations,
        tags: expanded,
        rows,
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Color {
    White,
    Gray,
    Black,
}

struct Dfs<'a> {
    direct: &'a [Vec<u32>],
    children: &'a [Vec<usize>],
    memo: Vec<Option<Vec<u32>>>,
    color: Vec<Color>,
}

impl Dfs<'_> {
    /// The tag's expanded artwork set (sorted, deduped), memoized. Recursion depth is
    /// bounded by the hierarchy depth (~10 in today's data; a Gray mark breaks cycles).
    fn subtree(&mut self, i: usize) -> &[u32] {
        if self.color[i] == Color::Gray {
            // Back-edge: we're already computing `i` somewhere up the stack. Treat the
            // cycle edge as empty; the in-flight computation still gathers the union.
            // Within a cycle this makes results order-dependent (a member memoized
            // before the loop closes can miss artworks only reachable back through
            // it) — accepted: real Tagger data is acyclic (verified against the live
            // bulk file), every tag always keeps at least its own direct taggings,
            // and the guard exists purely so a malformed file terminates.
            return &[];
        }
        if self.memo[i].is_none() {
            self.color[i] = Color::Gray;
            let mut set: HashSet<u32> = self.direct[i].iter().copied().collect();
            for k in 0..self.children[i].len() {
                let child = self.children[i][k];
                set.extend(self.subtree(child).iter().copied());
            }
            let mut ills: Vec<u32> = set.into_iter().collect();
            ills.sort_unstable();
            self.color[i] = Color::Black;
            self.memo[i] = Some(ills);
        }
        self.memo[i].as_deref().unwrap_or(&[])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(id: &str, slug: &str, children: &[&str], ills: &[&str]) -> TagInput {
        TagInput {
            scryfall_id: id.to_string(),
            slug: slug.to_string(),
            label: slug.to_string(),
            description: None,
            child_ids: children.iter().map(|c| c.to_string()).collect(),
            taggings: ills.iter().map(|i| i.to_string()).collect(),
        }
    }

    fn known(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|i| i.to_string()).collect()
    }

    /// The expanded artwork ids for `slug`, resolved back through the interner.
    fn ills_of<'a>(out: &'a Expanded, slug: &str) -> Vec<&'a str> {
        let tag = out
            .tags
            .iter()
            .find(|t| t.slug == slug)
            .unwrap_or_else(|| panic!("tag {slug} missing"));
        let mut ids: Vec<&str> = tag
            .illustrations
            .iter()
            .map(|&i| out.illustrations[i as usize].as_str())
            .collect();
        ids.sort_unstable();
        ids
    }

    #[test]
    fn direct_taggings_pass_through() {
        let out = expand(
            vec![tag("t1", "squirrel", &[], &["i1", "i2"])],
            &known(&["i1", "i2"]),
        );
        assert_eq!(ills_of(&out, "squirrel"), vec!["i1", "i2"]);
        assert_eq!(out.rows, 2);
    }

    #[test]
    fn parents_collect_descendants_and_dedupe() {
        // animal -> rodent -> squirrel; animal also tags i1 directly, squirrel tags
        // i1 too (shared artwork must not double-count).
        let out = expand(
            vec![
                tag("a", "animal", &["r"], &["i1"]),
                tag("r", "rodent", &["s"], &[]),
                tag("s", "squirrel", &[], &["i1", "i2"]),
            ],
            &known(&["i1", "i2"]),
        );
        assert_eq!(ills_of(&out, "animal"), vec!["i1", "i2"]);
        assert_eq!(ills_of(&out, "rodent"), vec!["i1", "i2"]);
        assert_eq!(ills_of(&out, "squirrel"), vec!["i1", "i2"]);
        assert_eq!(out.rows, 6);
    }

    #[test]
    fn multi_parent_subtrees_are_shared() {
        // Both parents pick up the same child's artworks.
        let out = expand(
            vec![
                tag("p1", "mammal", &["c"], &[]),
                tag("p2", "pet", &["c"], &[]),
                tag("c", "dog", &[], &["i1"]),
            ],
            &known(&["i1"]),
        );
        assert_eq!(ills_of(&out, "mammal"), vec!["i1"]);
        assert_eq!(ills_of(&out, "pet"), vec!["i1"]);
    }

    #[test]
    fn cycles_terminate_without_losing_taggings() {
        // a <-> b: pathological, but expansion must terminate and both keep their
        // (combined) direct taggings.
        let out = expand(
            vec![
                tag("a", "alpha", &["b"], &["i1"]),
                tag("b", "beta", &["a"], &["i2"]),
            ],
            &known(&["i1", "i2"]),
        );
        // Whichever DFS order, each tag keeps at least its own tagging and the pair
        // gathers both artworks between them.
        assert!(ills_of(&out, "alpha").contains(&"i1"));
        assert!(ills_of(&out, "beta").contains(&"i2"));
    }

    #[test]
    fn unknown_artworks_and_empty_tags_are_dropped() {
        let out = expand(
            vec![
                tag("d", "digital-only", &[], &["i9"]),
                tag("k", "kept", &[], &["i1", "i9"]),
                tag("e", "empty-parent", &["d"], &[]),
            ],
            &known(&["i1"]),
        );
        // i9 isn't a stored artwork: digital-only and empty-parent expand to nothing
        // and disappear entirely; kept retains only i1.
        assert_eq!(out.tags.len(), 1);
        assert_eq!(ills_of(&out, "kept"), vec!["i1"]);
        assert_eq!(out.rows, 1);
    }

    #[test]
    fn unknown_child_ids_are_ignored() {
        let out = expand(
            vec![tag("p", "parent", &["missing-id"], &["i1"])],
            &known(&["i1"]),
        );
        assert_eq!(ills_of(&out, "parent"), vec!["i1"]);
    }
}
