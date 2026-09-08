//! **The one definition of "a release worth listing."**
//!
//! Two surfaces read it: the day-before release heads-ups ([`crate::release_alerts`]) and the
//! public release calendar (`GET /api/games/{game}/releases`,
//! [`crate::handlers::catalog::list_releases`]). They were written months apart, and the
//! calendar could easily have grown a second, slightly different filter — a set-type list
//! missing `box`, a per-set path that still admits `sld` — and then shown a page that
//! disagrees with the notification a subscriber gets. So the predicate, the Secret Lair
//! classification and the per-drop date derivation live here, once, and each surface only
//! decides what to *do* with a release: notify, or render.
//!
//! What counts (issue #679, and the reasoning in the alert engine's own docs):
//!
//! - **Regular sets** carry `card_sets.released_at` directly. One entry per *theme*, never
//!   per sealed product: top-level sets only (`parent_set_code IS NULL`, so an expansion's
//!   tied Commander / token child sets fold into the one theme), a curated set-type
//!   allow-list ([`ANNOUNCEABLE_SET_TYPES`]), non-digital, and never the continuously
//!   restocked `sld` set itself (its drops are listed per drop below).
//! - **A set that is itself a Secret Lair release** — an `sl`-prefixed code filed as its own
//!   top-level set, the way The Zeta Set (`slz`) was — passes the same filters and is then
//!   *upgraded* by [`is_secret_lair_release`]. The prefix only ever upgrades a set that
//!   already passed; it never rescues one the filters drop.
//! - **Secret Lair drops** aren't dated in the bulk API, so a drop's street date is the
//!   earliest `released_at` among its cards, grouped through the runtime drop table
//!   ([`crate::scryfall::drops`]) — and read **only off `sld`**: a Zeta section is a print
//!   treatment, not a product or a separately dated release, so `slz` is a set here and
//!   never a drop.

use std::collections::HashMap;

use sea_orm::{
    ColumnTrait, DatabaseConnection, DbErr, EntityTrait, QueryFilter, QueryOrder, QuerySelect,
    Select,
};

use crate::entities::prelude::{Card, CardSet};
use crate::entities::{card, card_set};
use crate::scryfall::drops;

/// The game the Secret Lair surfaces cover (MTG). The per-drop path and the `sl`-prefix
/// classification are both Scryfall vocabulary, so every caller guards on this first.
pub const GAME: &str = crate::scryfall::GAME;

/// The Secret Lair set code (lowercased, as cards and sets store it) — the one set whose
/// releases are listed per *drop* rather than as a set.
pub const SLD_SET_CODE: &str = crate::mtgjson::sld::SET_CODE;

/// The set-code prefix Scryfall gives the Secret Lair family (`sld`, `slu`, `slc`, `slp`,
/// `slx`, `slz`, …). A set the set path admits whose code carries it is a Secret Lair release
/// of its own — see [`is_secret_lair_set_code`].
const SECRET_LAIR_CODE_PREFIX: &str = "sl";

/// The set types a release listing covers: the major retail themes a collector would want to
/// know about. Deliberately excludes the noise (tokens, promos, memorabilia, digital-only
/// alchemy, minigames, …) so an entry is a real release, not a same-day accessory printing.
///
/// `box` is on the list because that is how Scryfall filed The Zeta Set (`slz`, 2026-09-02): a
/// Secret Lair-line release published as its **own top-level `box` set** — no parent set, no
/// cards in `sld` — so the per-drop path (which reads `sld` cards only) never saw it and, with
/// `box` excluded here, neither did the set path: nobody was told. The `sld` set itself stays
/// out by code (its drops list per drop) and the `sld`-parented spin-offs (`slu`, `slc`) by the
/// top-level filter. The bucket is otherwise dormant: the catalog's other top-level `box` sets
/// (Game Night, Guild Kits, Challenger Decks, and older oddities such as the Salvat and
/// Hachette partworks) all released between 1996 and 2022, so in practice this entry admits a
/// future release like `slz` and whatever else Scryfall files as a top-level box.
pub const ANNOUNCEABLE_SET_TYPES: &[&str] = &[
    "core",
    "expansion",
    "commander",
    "draft_innovation",
    "masters",
    "funny",
    "box",
];

/// Whether a set code carries the Secret Lair family's `sl` prefix. Judged **after** the set
/// filters, never instead of them: `sld` itself is out by code, its spin-offs (`slu`, `slc`,
/// `slp`, `slx`) by their `sld` parent, and `slci` — The Lost Caverns of Ixalan's substitute
/// cards, a `token` child set — by both type and parent, so the prefix only ever upgrades a
/// set that would have been listed anyway. Case follows the catalog (codes are stored
/// lowercased).
pub fn is_secret_lair_set_code(code: &str) -> bool {
    code.starts_with(SECRET_LAIR_CODE_PREFIX)
}

/// Whether a set [`announceable_sets`] admitted is a Secret Lair release of its own — an MTG
/// set whose code carries the family prefix. The MTG guard is what keeps another game's
/// `sl…` code from being mislabelled.
pub fn is_secret_lair_release(set: &card_set::Model) -> bool {
    set.game == GAME && is_secret_lair_set_code(&set.code)
}

/// The sets releasing inside `[from, to]` (inclusive ISO `YYYY-MM-DD` bounds), across every
/// game, ordered by date then code. This is the predicate itself, returned as a query so a
/// caller can narrow it (the calendar adds its game) without restating a filter.
pub fn announceable_sets(from: &str, to: &str) -> Select<card_set::Entity> {
    CardSet::find()
        .filter(card_set::Column::ReleasedAt.gte(from))
        .filter(card_set::Column::ReleasedAt.lte(to))
        .filter(card_set::Column::Digital.eq(false))
        .filter(card_set::Column::ParentSetCode.is_null())
        .filter(card_set::Column::Code.ne(SLD_SET_CODE))
        .filter(
            card_set::Column::SetType.is_in(ANNOUNCEABLE_SET_TYPES.iter().map(|s| s.to_string())),
        )
        .order_by_asc(card_set::Column::ReleasedAt)
        .order_by_asc(card_set::Column::Code)
}

/// One Secret Lair drop with cards releasing inside a window, resolved to what a listing
/// needs. The `slug` is the drop's stable key (the drop table's, and what the set page's
/// by-drop view anchors on); `order` is its position in the snapshot's display order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SldDropRelease {
    pub slug: String,
    pub title: String,
    pub order: usize,
    /// ISO `YYYY-MM-DD`: the earliest in-window `released_at` among the drop's cards.
    pub released_at: String,
}

/// Secret Lair drops with cards releasing inside `[from, to]`, one entry per drop, in the
/// snapshot's display order. Cards are grouped to their drop via the runtime drop table, so a
/// card whose collector number isn't in the current snapshot (a not-yet-listed drop) is simply
/// skipped — the feature degrades to "not seen" rather than guessing. Reads **only `sld`**:
/// no other set's cards can produce a drop, by construction.
///
/// An absent drop table (no snapshot loaded yet) is an empty answer, not an error.
pub async fn sld_drops_releasing(
    db: &DatabaseConnection,
    from: &str,
    to: &str,
) -> Result<Vec<SldDropRelease>, DbErr> {
    let Some(table) = drops::table(GAME, SLD_SET_CODE) else {
        return Ok(Vec::new());
    };

    let rows: Vec<(String, Option<String>)> = Card::find()
        .select_only()
        .column(card::Column::CollectorNumber)
        .column(card::Column::ReleasedAt)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::SetCode.eq(SLD_SET_CODE))
        .filter(card::Column::ReleasedAt.gte(from))
        .filter(card::Column::ReleasedAt.lte(to))
        .into_tuple()
        .all(db)
        .await?;

    let mut by_slug: HashMap<String, SldDropRelease> = HashMap::new();
    for (collector_number, released_at) in rows {
        let Some(released_at) = released_at else {
            continue;
        };
        let Some(drop) = table.drop_for(&collector_number) else {
            continue;
        };
        match by_slug.get_mut(&drop.slug) {
            Some(release) => {
                if released_at < release.released_at {
                    release.released_at = released_at;
                }
            }
            None => {
                by_slug.insert(
                    drop.slug.clone(),
                    SldDropRelease {
                        slug: drop.slug.clone(),
                        title: drop.title.clone(),
                        order: drop.order,
                        released_at,
                    },
                );
            }
        }
    }

    let mut releases: Vec<SldDropRelease> = by_slug.into_values().collect();
    releases.sort_by_key(|release| release.order);
    Ok(releases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{card_set_model, migrated_memory_db};
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, Set};

    /// The Secret Lair family is recognised by its `sl` code prefix and nothing else — the set
    /// filters, not this predicate, keep `sld` and its child sets out.
    #[test]
    fn secret_lair_set_code_is_the_sl_prefix() {
        assert!(is_secret_lair_set_code("slz"));
        assert!(is_secret_lair_set_code("sld"));
        assert!(is_secret_lair_set_code("slu"));
        assert!(!is_secret_lair_set_code("tbox"));
        assert!(!is_secret_lair_set_code("s"));
        assert!(!is_secret_lair_set_code(""));
        // Case follows the catalog (codes are stored lowercased).
        assert!(!is_secret_lair_set_code("SLZ"));
    }

    /// The upgrade is MTG vocabulary: another game's `sl…` code is not a Secret Lair.
    #[test]
    fn secret_lair_release_is_gated_on_the_game() {
        let mtg = card_set::Model {
            game: GAME.to_string(),
            ..card_set_model("slz")
        };
        assert!(is_secret_lair_release(&mtg));
        let other = card_set::Model {
            game: "pkm".to_string(),
            ..card_set_model("slz")
        };
        assert!(!is_secret_lair_release(&other));
        let plain = card_set::Model {
            game: GAME.to_string(),
            ..card_set_model("blb")
        };
        assert!(!is_secret_lair_release(&plain));
    }

    async fn insert_set(
        db: &DatabaseConnection,
        code: &str,
        set_type: &str,
        released_at: &str,
        digital: bool,
        parent: Option<&str>,
    ) {
        let now = Utc::now();
        card_set::ActiveModel {
            game: Set(GAME.to_string()),
            code: Set(code.to_string()),
            name: Set(code.to_uppercase()),
            set_type: Set(Some(set_type.to_string())),
            released_at: Set(Some(released_at.to_string())),
            card_count: Set(0),
            digital: Set(digital),
            parent_set_code: Set(parent.map(str::to_string)),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// The set predicate, end to end: top-level, allow-listed type (including `box`), paper,
    /// never `sld`, inside the inclusive window, ordered by date then code.
    #[tokio::test]
    async fn announceable_sets_apply_every_filter() {
        let db = migrated_memory_db().await;
        insert_set(&db, "blb", "expansion", "2026-08-01", false, None).await;
        insert_set(&db, "blc", "commander", "2026-08-01", false, Some("blb")).await;
        insert_set(&db, "tblb", "token", "2026-08-01", false, Some("blb")).await;
        insert_set(&db, "slz", "box", "2026-08-02", false, None).await;
        insert_set(&db, "sld", "box", "2026-08-02", false, None).await;
        insert_set(&db, "slu", "box", "2026-08-02", false, Some("sld")).await;
        insert_set(&db, "ymid", "alchemy", "2026-08-03", true, None).await;
        insert_set(&db, "pblb", "promo", "2026-08-03", false, None).await;
        insert_set(&db, "aaa", "core", "2026-08-01", false, None).await;
        insert_set(&db, "old", "expansion", "2026-07-31", false, None).await;
        insert_set(&db, "late", "expansion", "2026-08-11", false, None).await;
        insert_set(&db, "edge", "masters", "2026-08-10", false, None).await;

        let codes: Vec<String> = announceable_sets("2026-08-01", "2026-08-10")
            .all(&db)
            .await
            .unwrap()
            .into_iter()
            .map(|set| set.code)
            .collect();
        assert_eq!(codes, ["aaa", "blb", "slz", "edge"]);
    }

    async fn insert_sld_card(db: &DatabaseConnection, collector_number: &str, released_at: &str) {
        let now = Utc::now();
        card::ActiveModel {
            game: Set(GAME.to_string()),
            external_id: Set(format!("sld-{collector_number}")),
            name: Set(format!("SLD {collector_number}")),
            set_code: Set(SLD_SET_CODE.to_string()),
            set_name: Set("Secret Lair Drop".to_string()),
            collector_number: Set(collector_number.to_string()),
            lang: Set("en".to_string()),
            released_at: Set(Some(released_at.to_string())),
            digital: Set(false),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
    }

    /// A drop's date is the earliest in-window date among its cards; cards outside the window
    /// or outside the snapshot contribute nothing.
    #[tokio::test]
    async fn sld_drops_group_cards_by_the_snapshot_and_window() {
        let db = migrated_memory_db().await;
        // #2658 and #2659 are in the committed snapshot's "Wild in Bloom" drop.
        insert_sld_card(&db, "2658", "2026-07-22").await;
        insert_sld_card(&db, "2659", "2026-07-21").await;
        // Outside the window: must not pull the drop's date earlier.
        insert_sld_card(&db, "2660", "2026-07-01").await;
        // A collector number no snapshot lists: skipped rather than guessed.
        insert_sld_card(&db, "999999", "2026-07-21").await;

        let releases = sld_drops_releasing(&db, "2026-07-20", "2026-07-31")
            .await
            .unwrap();
        assert_eq!(releases.len(), 1, "{releases:?}");
        let wild = &releases[0];
        assert_eq!(wild.title, "Wild in Bloom");
        assert_eq!(wild.released_at, "2026-07-21");

        let none = sld_drops_releasing(&db, "2026-08-01", "2026-08-31")
            .await
            .unwrap();
        assert!(none.is_empty());
    }
}
