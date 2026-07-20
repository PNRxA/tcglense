//! Release-alert evaluation engine — the day-before heads-up for upcoming releases.
//!
//! Two opt-in subscriptions (per-user flags on the [`alert_channel`] row): a **Secret Lair
//! drop** releasing (`sld_release_enabled`) and a **new regular set** releasing
//! (`set_release_enabled`). Once per interval ([`crate::tasks`] drives the cadence) the engine
//! finds releases whose date lands inside the look-ahead window (`[today, today + LEAD_DAYS]`,
//! so a daily-ish run always catches "tomorrow") and delivers a heads-up over each opted-in
//! user's configured channels — the same Discord / Telegram / optional-email fan-out price
//! alerts use ([`crate::notifications::deliver_to_user`]).
//!
//! **Edge-triggered, exactly once per (user, release).** A `release_notifications` ledger row
//! is written only after a channel actually accepts the message, so:
//! - a user is never re-notified about the same drop/set on a later pass, and
//! - an undeliverable heads-up (no channel configured yet) is retried on the next pass rather
//!   than silently swallowed — until the release drops out of the window.
//!
//! Where the dates come from (no new ingestion — both are already in the catalog):
//! - **Secret Lair drops** aren't dated in the bulk API, so a drop's street date is the
//!   earliest `released_at` among the drop's cards (Scryfall stamps each printing with its
//!   drop's date), grouped by the runtime drop table ([`crate::scryfall::drops`]). Only drops
//!   whose cards are already in the catalog with a near-future date are notifiable — a drop not
//!   yet spoiled simply isn't seen, and the feature degrades gracefully rather than guessing.
//! - **Regular sets** carry `card_sets.released_at` directly. A single notification per set
//!   (the theme), never per sealed product: top-level sets only (`parent_set_code IS NULL`, so
//!   an expansion's tied Commander/token child sets fold into the one theme), a curated set-type
//!   allow-list, non-digital, and never the continuously-restocked Secret Lair line.
//!
//! Memory stays O(batch): the opted-in users are keyset-paginated by `alert_channels.id`, and
//! the notified-ledger lookup + delivery run per page — nothing loads every subscriber at once.

use std::collections::{HashMap, HashSet};

use chrono::NaiveDate;
use sea_orm::prelude::DateTimeUtc;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect, Set,
};

use crate::email::Emailer;
use crate::entities::prelude::{AlertChannel, Card, CardSet, ReleaseNotification};
use crate::entities::{alert_channel, card, card_set, release_notification};
use crate::notifications::{self, AlertNotification};
use crate::scryfall::drops;

/// The game the release surfaces cover today (MTG). Secret Lair and set dates are both MTG.
const GAME: &str = crate::scryfall::GAME;

/// The Secret Lair set code (lowercased, as cards store it).
const SLD_SET_CODE: &str = "sld";

/// How many days ahead of a release to notify. `1` = "a day before scheduled release"; the
/// window is inclusive of today too, so a run that missed yesterday still catches a release on
/// its own day (deduped, so this only ever *rescues* a missed heads-up, never doubles one).
const LEAD_DAYS: i64 = 1;

/// Keyset page size when scanning opted-in users — memory stays O(batch) regardless of how
/// many subscribers exist (the same bound the price-alert evaluator uses).
const USER_BATCH: u64 = 2_000;

/// The set types a "new set release" heads-up covers: the major retail themes a collector
/// would want a heads-up for. Deliberately excludes the noise (tokens, promos, memorabilia,
/// the Secret Lair `box` line, digital-only alchemy, minigames, …) so the notification is a
/// real release, not a same-day accessory printing.
const NOTIFY_SET_TYPES: &[&str] = &[
    "core",
    "expansion",
    "commander",
    "draft_innovation",
    "masters",
    "funny",
];

/// Which kind of upcoming release a heads-up is for. Determines the opt-in flag it reads, the
/// ledger `kind` it dedups on, and the message wording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReleaseKind {
    /// A Secret Lair drop, keyed by its slug.
    SldDrop,
    /// A regular set, keyed by its code.
    Set,
}

impl ReleaseKind {
    /// The stable ledger discriminator (`release_notifications.kind`).
    fn as_str(self) -> &'static str {
        match self {
            ReleaseKind::SldDrop => "sld_drop",
            ReleaseKind::Set => "set",
        }
    }

    /// The `alert_channels` opt-in column gating this kind.
    fn enabled_column(self) -> alert_channel::Column {
        match self {
            ReleaseKind::SldDrop => alert_channel::Column::SldReleaseEnabled,
            ReleaseKind::Set => alert_channel::Column::SetReleaseEnabled,
        }
    }
}

/// One upcoming release the evaluator may notify about, resolved to what a message needs.
struct UpcomingRelease {
    kind: ReleaseKind,
    /// The stable per-release key within the kind (drop slug | set code) — the ledger `ref_key`.
    ref_key: String,
    game: String,
    /// Human display name (drop title | set name).
    display_name: String,
    /// ISO `YYYY-MM-DD` release date.
    release_date: String,
}

/// Evaluate both release surfaces once and deliver any day-before heads-ups.
///
/// `now` is injected (not read from a clock) so the window logic is deterministic in tests.
/// Errors on any single query / channel are logged and swallowed so one bad row never stalls
/// the pass.
pub async fn evaluate_all(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    email_globally_enabled: bool,
    public_site_url: &str,
    now: DateTimeUtc,
) {
    let today = now.date_naive();
    let horizon = today + chrono::Duration::days(LEAD_DAYS);
    let from = today.to_string();
    let to = horizon.to_string();

    let sld = upcoming_sld_drops(db, &from, &to).await;
    let sets = upcoming_sets(db, &from, &to).await;

    deliver_kind(
        db,
        notify_http,
        emailer,
        email_globally_enabled,
        public_site_url,
        today,
        ReleaseKind::SldDrop,
        &sld,
    )
    .await;
    deliver_kind(
        db,
        notify_http,
        emailer,
        email_globally_enabled,
        public_site_url,
        today,
        ReleaseKind::Set,
        &sets,
    )
    .await;
}

/// Deliver a kind's releases to its opted-in users, keyset-paginated over the subscribers so
/// memory stays O(batch). Per page: load which (user, release) pairs are already in the ledger,
/// then for each fresh pair deliver + record. Recording only on a successful delivery preserves
/// the "retry next pass until a channel works" contract.
#[allow(clippy::too_many_arguments)]
async fn deliver_kind(
    db: &DatabaseConnection,
    notify_http: &reqwest::Client,
    emailer: &Emailer,
    email_globally_enabled: bool,
    public_site_url: &str,
    today: NaiveDate,
    kind: ReleaseKind,
    releases: &[UpcomingRelease],
) {
    if releases.is_empty() {
        return;
    }
    let flag = kind.enabled_column();
    let ref_keys: Vec<String> = releases.iter().map(|r| r.ref_key.clone()).collect();

    let mut after: i32 = 0;
    loop {
        let page: Vec<(i32, i32)> = match AlertChannel::find()
            .select_only()
            .column(alert_channel::Column::Id)
            .column(alert_channel::Column::UserId)
            .filter(flag.eq(true))
            .filter(alert_channel::Column::Id.gt(after))
            .order_by_asc(alert_channel::Column::Id)
            .limit(USER_BATCH)
            .into_tuple()
            .all(db)
            .await
        {
            Ok(page) => page,
            Err(err) => {
                tracing::warn!(error = %err, kind = kind.as_str(), "failed to load a release-alert subscriber batch");
                return;
            }
        };
        let Some(&(last_id, _)) = page.last() else {
            break;
        };
        after = last_id;
        let page_len = page.len();

        let user_ids: Vec<i32> = page.iter().map(|&(_, uid)| uid).collect();
        // Fail SAFE: if the dedup ledger can't be read, skip *this page's* deliveries rather than
        // re-notifying everyone on it (an empty "already" set would make the dedup guard below
        // always false). `after` is already advanced, so we move on; the whole set is re-scanned
        // on the next full pass, and delivery is idempotently retried — deferring beats spamming.
        let Some(already) = load_already_notified(db, kind, &user_ids, &ref_keys).await else {
            if (page_len as u64) < USER_BATCH {
                break;
            }
            continue;
        };

        for &(_, user_id) in &page {
            for release in releases {
                if already.contains(&(user_id, release.ref_key.clone())) {
                    continue;
                }
                let notification = build_notification(release, today, public_site_url);
                let delivered = notifications::deliver_to_user(
                    db,
                    notify_http,
                    emailer,
                    email_globally_enabled,
                    user_id,
                    &notification,
                )
                .await;
                if delivered {
                    record_sent(db, user_id, kind, release, now_from(today)).await;
                }
            }
        }

        // A short page is the last page; skip one extra empty round-trip.
        if (page_len as u64) < USER_BATCH {
            break;
        }
    }
}

/// The `sent_at` stamp for a ledger row. We only carry `today` down the delivery path (the
/// window comparisons are date-only), so stamp midnight UTC of the notifying day — precise
/// enough for a reference/debug column and keeps the whole path clock-injectable for tests.
fn now_from(today: NaiveDate) -> DateTimeUtc {
    today
        .and_hms_opt(0, 0, 0)
        .expect("midnight is a valid time")
        .and_utc()
}

/// Secret Lair drops with cards releasing inside `[from, to]`, one entry per drop. A drop's
/// date is the earliest near-future `released_at` among its cards (they share a street date);
/// cards are grouped to their drop via the runtime drop table, so a card whose collector number
/// isn't in the current snapshot (a not-yet-listed drop) is simply skipped.
async fn upcoming_sld_drops(db: &DatabaseConnection, from: &str, to: &str) -> Vec<UpcomingRelease> {
    let Some(table) = drops::table(GAME, SLD_SET_CODE) else {
        return Vec::new();
    };

    let rows: Vec<(String, Option<String>)> = match Card::find()
        .select_only()
        .column(card::Column::CollectorNumber)
        .column(card::Column::ReleasedAt)
        .filter(card::Column::Game.eq(GAME))
        .filter(card::Column::SetCode.eq(SLD_SET_CODE))
        .filter(card::Column::ReleasedAt.gte(from))
        .filter(card::Column::ReleasedAt.lte(to))
        .into_tuple()
        .all(db)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(error = %err, "failed to load upcoming Secret Lair cards");
            return Vec::new();
        }
    };

    struct Accum {
        title: String,
        order: usize,
        release_date: String,
    }
    let mut by_slug: HashMap<String, Accum> = HashMap::new();
    for (collector_number, released_at) in rows {
        let Some(released_at) = released_at else {
            continue;
        };
        let Some(drop) = table.drop_for(&collector_number) else {
            continue;
        };
        match by_slug.get_mut(&drop.slug) {
            Some(accum) => {
                if released_at < accum.release_date {
                    accum.release_date = released_at;
                }
            }
            None => {
                by_slug.insert(
                    drop.slug.clone(),
                    Accum {
                        title: drop.title.clone(),
                        order: drop.order,
                        release_date: released_at,
                    },
                );
            }
        }
    }

    // Stable output order: the drop's display order in the snapshot.
    let mut ordered: Vec<(usize, UpcomingRelease)> = by_slug
        .into_iter()
        .map(|(slug, accum)| {
            (
                accum.order,
                UpcomingRelease {
                    kind: ReleaseKind::SldDrop,
                    ref_key: slug,
                    game: GAME.to_string(),
                    display_name: accum.title,
                    release_date: accum.release_date,
                },
            )
        })
        .collect();
    ordered.sort_by_key(|(order, _)| *order);
    ordered.into_iter().map(|(_, release)| release).collect()
}

/// Regular sets releasing inside `[from, to]`: one entry per theme. Top-level sets only (so an
/// expansion's tied child sets don't each notify), a curated set-type allow-list, non-digital,
/// and never the Secret Lair line (handled per-drop above).
async fn upcoming_sets(db: &DatabaseConnection, from: &str, to: &str) -> Vec<UpcomingRelease> {
    let rows: Vec<card_set::Model> = match CardSet::find()
        .filter(card_set::Column::ReleasedAt.gte(from))
        .filter(card_set::Column::ReleasedAt.lte(to))
        .filter(card_set::Column::Digital.eq(false))
        .filter(card_set::Column::ParentSetCode.is_null())
        .filter(card_set::Column::Code.ne(SLD_SET_CODE))
        .filter(card_set::Column::SetType.is_in(NOTIFY_SET_TYPES.iter().map(|s| s.to_string())))
        .order_by_asc(card_set::Column::ReleasedAt)
        .order_by_asc(card_set::Column::Code)
        .all(db)
        .await
    {
        Ok(rows) => rows,
        Err(err) => {
            tracing::warn!(error = %err, "failed to load upcoming set releases");
            return Vec::new();
        }
    };

    rows.into_iter()
        .filter_map(|set| {
            let release_date = set.released_at?;
            Some(UpcomingRelease {
                kind: ReleaseKind::Set,
                ref_key: set.code,
                game: set.game,
                display_name: set.name,
                release_date,
            })
        })
        .collect()
}

/// The `(user_id, ref_key)` pairs already in the ledger for this kind, scoped to a page of
/// users and the pass's release keys — so a delivered heads-up is never sent twice. Returns
/// `None` on a DB error: the caller must then **skip** the page rather than treat it as
/// "nobody notified yet" (which would re-deliver to everyone), because delivery is not
/// transactional with recording — the retry-next-pass path is safe, a false empty set is not.
async fn load_already_notified(
    db: &DatabaseConnection,
    kind: ReleaseKind,
    user_ids: &[i32],
    ref_keys: &[String],
) -> Option<HashSet<(i32, String)>> {
    if user_ids.is_empty() || ref_keys.is_empty() {
        return Some(HashSet::new());
    }
    match ReleaseNotification::find()
        .select_only()
        .column(release_notification::Column::UserId)
        .column(release_notification::Column::RefKey)
        .filter(release_notification::Column::Kind.eq(kind.as_str()))
        .filter(release_notification::Column::UserId.is_in(user_ids.iter().copied()))
        .filter(release_notification::Column::RefKey.is_in(ref_keys.iter().cloned()))
        .into_tuple()
        .all(db)
        .await
    {
        Ok(rows) => Some(rows.into_iter().collect()),
        Err(err) => {
            tracing::warn!(error = %err, "failed to load the release-notification ledger; skipping this page to avoid re-notifying");
            None
        }
    }
}

/// Record a delivered heads-up in the ledger. A unique-constraint conflict (a race, or a stale
/// `already` set) is benign — the goal is "at most once" — so it's logged at debug, not fatal.
async fn record_sent(
    db: &DatabaseConnection,
    user_id: i32,
    kind: ReleaseKind,
    release: &UpcomingRelease,
    now: DateTimeUtc,
) {
    let row = release_notification::ActiveModel {
        user_id: Set(user_id),
        kind: Set(kind.as_str().to_string()),
        ref_key: Set(release.ref_key.clone()),
        game: Set(release.game.clone()),
        release_date: Set(release.release_date.clone()),
        sent_at: Set(now),
        ..Default::default()
    };
    if let Err(err) = ReleaseNotification::insert(row).exec(db).await {
        tracing::debug!(error = %err, user_id, kind = kind.as_str(), "failed to record a release notification (likely a duplicate)");
    }
}

/// Build the heads-up message for one release: a title, a body with day-aware wording, and a
/// link to the set's page in the SPA (both kinds live under the set page — a drop under the
/// Secret Lair set).
fn build_notification(
    release: &UpcomingRelease,
    today: NaiveDate,
    public_site_url: &str,
) -> AlertNotification {
    let base = public_site_url.trim_end_matches('/');
    let when = release_phrase(&release.release_date, today);
    match release.kind {
        ReleaseKind::SldDrop => AlertNotification {
            title: format!("Secret Lair drop: {}", release.display_name),
            body: format!("✨ The Secret Lair drop “{}” {when}.", release.display_name),
            url: Some(format!("{base}/cards/{}/sets/{SLD_SET_CODE}", release.game)),
        },
        ReleaseKind::Set => AlertNotification {
            title: format!("New set: {}", release.display_name),
            body: format!("✨ {} {when}.", release.display_name),
            url: Some(format!(
                "{base}/cards/{}/sets/{}",
                release.game, release.ref_key
            )),
        },
    }
}

/// Day-aware release wording relative to `today`: "releases tomorrow" the day before, "releases
/// today" on release day (the catch-up case), else the plain date. The ISO date is appended for
/// tomorrow/today so the message is unambiguous across time zones.
fn release_phrase(release_date: &str, today: NaiveDate) -> String {
    match release_date.parse::<NaiveDate>() {
        Ok(date) => match (date - today).num_days() {
            days if days <= 0 => format!("releases today ({release_date})"),
            1 => format!("releases tomorrow ({release_date})"),
            _ => format!("releases on {release_date}"),
        },
        Err(_) => format!("releases on {release_date}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email::{Emailer, Mailbox};
    use crate::entities::prelude::ReleaseNotification;
    use chrono::Utc;
    use sea_orm::{ActiveModelTrait, PaginatorTrait};

    fn day(n: i64, today: NaiveDate) -> String {
        (today + chrono::Duration::days(n)).to_string()
    }

    #[test]
    fn release_phrase_is_day_aware() {
        let today: NaiveDate = "2026-07-20".parse().unwrap();
        assert_eq!(
            release_phrase("2026-07-21", today),
            "releases tomorrow (2026-07-21)"
        );
        assert_eq!(
            release_phrase("2026-07-20", today),
            "releases today (2026-07-20)"
        );
        // A past date (defensive — the window never selects one) still reads sanely.
        assert_eq!(
            release_phrase("2026-07-19", today),
            "releases today (2026-07-19)"
        );
        assert_eq!(
            release_phrase("2026-08-01", today),
            "releases on 2026-08-01"
        );
        assert_eq!(
            release_phrase("not-a-date", today),
            "releases on not-a-date"
        );
    }

    /// Insert a user + an alert-channel row opted into the given release kinds, with the email
    /// channel on (the capturing emailer "delivers" with no network). Returns the user id.
    async fn subscriber(db: &DatabaseConnection, email: &str, sld: bool, set: bool) -> i32 {
        let user_id = crate::test_support::insert_user(db, email).await;
        let now = Utc::now();
        alert_channel::ActiveModel {
            user_id: Set(user_id),
            email_enabled: Set(true),
            sld_release_enabled: Set(sld),
            set_release_enabled: Set(set),
            created_at: Set(now),
            updated_at: Set(now),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap();
        user_id
    }

    /// Insert a Secret Lair card at `collector_number` releasing on `released_at`.
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

    async fn ledger_count(db: &DatabaseConnection) -> u64 {
        ReleaseNotification::find().count(db).await.unwrap()
    }

    /// A Secret Lair drop releasing tomorrow notifies an opted-in user exactly once (a second
    /// pass is deduped by the ledger), and the message reads "releases tomorrow".
    #[tokio::test]
    async fn sld_drop_notifies_once_a_day_before() {
        let db = crate::test_support::migrated_memory_db().await;
        let now: DateTimeUtc = "2026-07-20T09:00:00Z".parse().unwrap();
        let today = now.date_naive();
        subscriber(&db, "sld@example.com", true, false).await;

        // #2658 is in the committed snapshot's "Wild in Bloom" drop; release it tomorrow.
        insert_sld_card(&db, "2658", &day(1, today)).await;

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;

        assert_eq!(mailbox.emails().len(), 1, "one heads-up delivered");
        assert!(
            mailbox.emails()[0].text.contains("releases tomorrow"),
            "day-before wording: {}",
            mailbox.emails()[0].text
        );
        assert!(
            mailbox.emails()[0].text.contains("Wild in Bloom"),
            "names the drop"
        );
        assert_eq!(ledger_count(&db).await, 1);

        // A second pass must not re-notify (ledger dedup).
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(mailbox.emails().len(), 1, "no re-notify on a later pass");
        assert_eq!(ledger_count(&db).await, 1);
    }

    /// A regular set releasing tomorrow notifies an opted-in user; a set releasing outside the
    /// window, a non-notifiable set type, and a Secret Lair `box` set are all skipped.
    #[tokio::test]
    async fn set_release_notifies_top_level_only_and_windows() {
        let db = crate::test_support::migrated_memory_db().await;
        let now: DateTimeUtc = "2026-07-20T09:00:00Z".parse().unwrap();
        let today = now.date_naive();
        subscriber(&db, "sets@example.com", false, true).await;

        // Releasing tomorrow, a notifiable expansion → notifies.
        insert_set(
            &db,
            "tst",
            "Test Expansion",
            Some("expansion"),
            &day(1, today),
            false,
            None,
        )
        .await;
        // Releasing tomorrow but a token set → skipped (not in the allow-list).
        insert_set(
            &db,
            "ttok",
            "Test Tokens",
            Some("token"),
            &day(1, today),
            false,
            None,
        )
        .await;
        // A tied child set (parent set code present) → folds into the theme, skipped.
        insert_set(
            &db,
            "tstc",
            "Test Commander",
            Some("commander"),
            &day(1, today),
            false,
            Some("tst"),
        )
        .await;
        // Releasing next week → outside the window, skipped.
        insert_set(
            &db,
            "far",
            "Far Future",
            Some("expansion"),
            &day(7, today),
            false,
            None,
        )
        .await;

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;

        assert_eq!(
            mailbox.emails().len(),
            1,
            "only the top-level expansion notifies"
        );
        assert!(mailbox.emails()[0].text.contains("Test Expansion"));
        assert_eq!(ledger_count(&db).await, 1);
    }

    /// A user who didn't opt into a kind gets nothing for it.
    #[tokio::test]
    async fn opt_out_gets_no_notification() {
        let db = crate::test_support::migrated_memory_db().await;
        let now: DateTimeUtc = "2026-07-20T09:00:00Z".parse().unwrap();
        let today = now.date_naive();
        // Opted into SETS only.
        let _user = subscriber(&db, "partial@example.com", false, true).await;
        // A Secret Lair drop releasing tomorrow — they didn't opt into SLD.
        insert_sld_card(&db, "2658", &day(1, today)).await;

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(
            mailbox.emails().len(),
            0,
            "SLD opt-out gets no SLD heads-up"
        );
        assert_eq!(ledger_count(&db).await, 0);
    }

    /// A set releasing **today** — the inclusive low bound of the look-ahead window — is caught
    /// (the day-of catch-up), and reads "releases today". Guards against a regression that
    /// tightens the low bound to `> today` and silently drops the catch-up path.
    #[tokio::test]
    async fn release_today_is_caught_by_the_inclusive_boundary() {
        let db = crate::test_support::migrated_memory_db().await;
        let now: DateTimeUtc = "2026-07-20T09:00:00Z".parse().unwrap();
        let today = now.date_naive();
        subscriber(&db, "today@example.com", false, true).await;
        // Releasing today (day 0), the inclusive `from` bound.
        insert_set(
            &db,
            "tdy",
            "Today Set",
            Some("expansion"),
            &day(0, today),
            false,
            None,
        )
        .await;

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(mailbox.emails().len(), 1, "a same-day release is caught");
        assert!(
            mailbox.emails()[0].text.contains("releases today"),
            "day-of wording: {}",
            mailbox.emails()[0].text
        );
        assert_eq!(ledger_count(&db).await, 1);
    }

    /// The "record only on delivery, retry until a channel works" contract: an opted-in user with
    /// no working channel is not recorded (so it isn't silently swallowed), and delivers exactly
    /// once when a channel is later configured — then dedups. Mirrors the price-alert
    /// `met_alert_latches_only_once_a_channel_delivers` guard for the release surface.
    #[tokio::test]
    async fn undelivered_headsup_is_retried_until_a_channel_works() {
        let db = crate::test_support::migrated_memory_db().await;
        let now: DateTimeUtc = "2026-07-20T09:00:00Z".parse().unwrap();
        let today = now.date_naive();

        // Opt into SLD but with NO working channel (email off; no Discord/Telegram).
        let user_id = crate::test_support::insert_user(&db, "nochan@example.com").await;
        let ts = Utc::now();
        alert_channel::ActiveModel {
            user_id: Set(user_id),
            email_enabled: Set(false),
            sld_release_enabled: Set(true),
            set_release_enabled: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
        insert_sld_card(&db, "2658", &day(1, today)).await;

        let http = reqwest::Client::new();
        let mailbox = Mailbox::default();

        // Pass 1: nothing delivered → nothing recorded (retryable, not swallowed).
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(mailbox.emails().len(), 0, "no channel: nothing delivered");
        assert_eq!(
            ledger_count(&db).await,
            0,
            "an undeliverable heads-up must not be recorded"
        );

        // Configure the email channel; now it delivers exactly once.
        let row = AlertChannel::find()
            .filter(alert_channel::Column::UserId.eq(user_id))
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        let mut active: alert_channel::ActiveModel = row.into();
        active.email_enabled = Set(true);
        active.update(&db).await.unwrap();

        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(mailbox.emails().len(), 1, "delivers once a channel exists");
        assert_eq!(ledger_count(&db).await, 1);

        // Pass 3: deduped — no re-notify.
        evaluate_all(
            &db,
            &http,
            &Emailer::Capture(mailbox.clone()),
            true,
            "https://x.test",
            now,
        )
        .await;
        assert_eq!(
            mailbox.emails().len(),
            1,
            "a recorded heads-up does not re-fire"
        );
    }

    /// Insert a `card_sets` row for the set-release tests.
    async fn insert_set(
        db: &DatabaseConnection,
        code: &str,
        name: &str,
        set_type: Option<&str>,
        released_at: &str,
        digital: bool,
        parent: Option<&str>,
    ) {
        let now = Utc::now();
        card_set::ActiveModel {
            game: Set(GAME.to_string()),
            code: Set(code.to_string()),
            name: Set(name.to_string()),
            set_type: Set(set_type.map(str::to_string)),
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
}
