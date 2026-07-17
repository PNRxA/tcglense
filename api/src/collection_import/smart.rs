//! The pure smart-sync page-absorption algorithm — the early-stop decision that lets a
//! smart fetch stop once the recently-updated prefix of a provider collection is already
//! in sync. No I/O, so the whole stop algorithm is unit-tested without the network. The
//! provider fetch loops (`archidekt::fetch_smart`, `moxfield::fetch_smart`) call this per
//! page.

use std::collections::HashMap;

use super::FetchedHolding;

/// Fold one provider page's normalized holdings (`(external_card_id, foil, quantity)`)
/// into a smart fetch's running state: append each to `holdings`, accumulate the
/// per-card running aggregate into `running` (`uid -> (regular, foil)`), and report
/// whether **every** card touched on this page now already equals its `local` count.
///
/// That "all match" flag is the smart stop signal: because the provider returns rows
/// most-recently-updated first, once a whole page is already in sync the rest of the
/// collection (updated even longer ago) is too, so paging can stop. The match is judged
/// only **after** the whole page is folded in, so a card that owns both a regular and a
/// foil finish isn't seen mid-aggregate just because its two rows sit on the same page.
///
/// The stop needs more than the current page, though: the provider can split one printing
/// across rows (differing condition/language/tags) whose `updatedAt`s put them on
/// non-adjacent pages, so a card can be mid-aggregate from an earlier page yet absent from
/// the page that looks in sync. Stopping there would strand its remaining rows and let the
/// reconcile overwrite it *down* to the partial count (silently dropping copies). So the
/// stop also requires that **no** card in the running aggregate is still below its local
/// count in either finish — a straddling (or genuinely decreased) card keeps paging until
/// its rows all land. Pure (no I/O) so the decision is unit-tested without the network.
///
/// `remap` folds a separately-modelled foil printing (`…★`) onto its base card as a foil
/// copy (issue #209) **before** aggregating, so the running aggregate, the accumulated
/// holdings, and the early-stop comparison all speak the base external id — and so the
/// holdings this returns are already consolidated for the reconcile.
pub(super) fn smart_absorb_page(
    running: &mut HashMap<String, (i64, i64)>,
    holdings: &mut Vec<FetchedHolding>,
    local: &HashMap<String, (i32, i32)>,
    remap: &HashMap<String, String>,
    page_rows: impl IntoIterator<Item = (String, bool, i32)>,
) -> bool {
    let mut touched: Vec<String> = Vec::new();
    for (uid, foil, quantity) in page_rows {
        // Fold a foil-★ variant onto its base as foil before it enters the aggregate.
        let (uid, foil) = match remap.get(&uid) {
            Some(base) => (base.clone(), true),
            None => (uid, foil),
        };
        let entry = running.entry(uid.clone()).or_insert((0, 0));
        let q = i64::from(quantity.max(0));
        if foil {
            entry.1 += q;
        } else {
            entry.0 += q;
        }
        touched.push(uid.clone());
        holdings.push(FetchedHolding {
            external_card_id: uid,
            foil,
            quantity,
        });
    }
    // A card matches only once its full running aggregate equals the local counts; an
    // unowned card (no local entry) never matches, so a new card keeps paging. A page
    // that contributed NO rows (e.g. Moxfield's fetch skips proxies / id-less custom
    // cards, so a bulk-edited block of proxies can fill a whole page) proves nothing
    // about sync state — vacuous truth here would falsely stop the fetch, so an empty
    // contribution reads as "keep paging".
    let page_in_sync = !touched.is_empty()
        && touched.iter().all(|uid| {
            running.get(uid).copied() == local.get(uid).map(|&(r, f)| (i64::from(r), i64::from(f)))
        });
    // ...but the per-page check above can't see a card whose rows straddle a page boundary:
    // seen (mid-aggregate, under-counted) on an earlier page, absent from this one. Stopping
    // while any card in the whole running aggregate is still *below* its local count would
    // strand that card's remaining rows and let reconcile_smart overwrite its finish down to
    // the partial count. Gate the stop on that too (checked only once the cheap per-page test
    // passes). A new card (local read as (0,0)) can't be below, so it never holds paging open.
    page_in_sync
        && running.iter().all(|(uid, &(reg, foil))| {
            let (lr, lf) = local
                .get(uid)
                .map_or((0, 0), |&(r, f)| (i64::from(r), i64::from(f)));
            reg >= lr && foil >= lf
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collection_import::{Provider, reconcile_smart};
    use crate::test_support::{
        insert_card, insert_holding, insert_user, migrated_memory_db, owned_counts,
    };

    #[test]
    fn smart_absorb_page_reports_all_match_only_when_page_is_in_sync() {
        let local = HashMap::from([("a".to_string(), (2, 0)), ("b".to_string(), (1, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        // Every card on the page equals local (b spans a regular + a foil row).
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![
                ("a".to_string(), false, 2),
                ("b".to_string(), false, 1),
                ("b".to_string(), true, 1),
            ],
        );
        assert!(all, "the page is fully in sync -> stop signal");
        assert_eq!(holdings.len(), 3, "every fetched row is still captured");
    }

    #[test]
    fn smart_absorb_page_flags_a_changed_or_new_card() {
        let local = HashMap::from([("a".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        // 'a' changed (3 != 2) and 'x' is unowned locally -> keep paging.
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 3), ("x".to_string(), false, 1)],
        );
        assert!(!all);
    }

    #[test]
    fn smart_absorb_page_treats_an_empty_contribution_as_keep_paging() {
        // A page whose rows were ALL filtered out upstream (e.g. Moxfield proxies /
        // id-less custom cards) contributes nothing — it must not read as "in sync"
        // (vacuous truth), or a bulk-edited block of proxies at the front of a smart
        // fetch would falsely stop it (or empty the whole fetch into an
        // EmptyCollection error for a non-empty collection).
        let local = HashMap::from([("a".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            Vec::new(),
        );
        assert!(!all, "an empty page contribution must keep paging");
        assert!(holdings.is_empty());
    }

    #[test]
    fn smart_absorb_page_defers_match_until_a_split_finish_settles() {
        // A card owned as regular + foil whose rows land on different pages: the first
        // page (regular only) reads as a mismatch because the running foil is still 0;
        // the second page (its foil row) settles the aggregate and matches.
        let local = HashMap::from([("a".to_string(), (2, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 2)],
        );
        assert!(!page1, "regular-only aggregate (2,0) != local (2,1)");
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), true, 1)],
        );
        assert!(page2, "now (2,1) == local -> stop signal");
    }

    #[test]
    fn smart_absorb_page_folds_a_foil_star_onto_its_base_and_matches_local() {
        // A held foil-★ (issue #209): the local snapshot has been consolidated to the base
        // as foil, and the re-fetched star row — even reported as a non-foil finish — folds
        // onto the same base, so the page reads as in sync.
        let remap = HashMap::from([("star".to_string(), "base".to_string())]);
        let local = HashMap::from([("base".to_string(), (0, 1))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();
        let all = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &remap,
            vec![("star".to_string(), false, 1)],
        );
        assert!(
            all,
            "the folded star matches the consolidated local -> stop signal"
        );
        assert_eq!(
            holdings,
            vec![FetchedHolding {
                external_card_id: "base".to_string(),
                foil: true,
                quantity: 1,
            }],
            "the captured holding is already remapped to the base as foil"
        );
    }

    #[test]
    fn smart_absorb_page_keeps_paging_while_an_earlier_card_is_still_below_local() {
        // Regression: a card whose provider rows straddle a page boundary must not be
        // stranded. `a` is owned as 2 regular but only one of its rows is on page 1; page 2's
        // own card (`b`) is fully in sync but `a` is absent from it. The old per-page-only
        // check stopped on page 2 and abandoned `a`'s second row, so reconcile then overwrote
        // `a` down to 1 (losing a copy). The stop must now be withheld while `a` is still
        // below its local count.
        let local = HashMap::from([("a".to_string(), (2, 0)), ("b".to_string(), (1, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();

        // Page 1: one of a's two regular rows -> a is mid-aggregate (1 of 2).
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 1)],
        );
        assert!(!page1, "a under-counted (1 != 2) -> keep paging");

        // Page 2: b is fully in sync and a is ABSENT — but a is still below local, so the
        // fetch must keep paging rather than strand a's remaining row.
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("b".to_string(), false, 1)],
        );
        assert!(
            !page2,
            "a still below its local count -> must not stop and strand its tail row"
        );

        // Page 3: a's second row lands; now the whole aggregate matches local -> stop.
        let page3 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("a".to_string(), false, 1)],
        );
        assert!(
            page3,
            "a now fully aggregated (2,0) and everything matches -> stop"
        );
    }

    #[test]
    fn smart_absorb_page_still_stops_when_a_card_grew_above_local() {
        // The below-local gate uses `>=`, not `==`: a card whose upstream count GREW (running
        // above its old local) is fully observed on the front pages, so it must NOT hold the
        // fetch open — otherwise any sync with a pending increase would degrade to a full scan.
        let local = HashMap::from([("grew".to_string(), (1, 0)), ("same".to_string(), (2, 0))]);
        let mut running = HashMap::new();
        let mut holdings = Vec::new();

        // Page 1: the grown card (now 3 regular). Above local, so this page isn't "in sync".
        let page1 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("grew".to_string(), false, 3)],
        );
        assert!(
            !page1,
            "grew (3,0) != local (1,0) on its own page -> keep paging"
        );

        // Page 2: an unchanged card, in sync. `grew` is above (not below) local, so it must
        // not block the stop.
        let page2 = smart_absorb_page(
            &mut running,
            &mut holdings,
            &local,
            &HashMap::new(),
            vec![("same".to_string(), false, 2)],
        );
        assert!(
            page2,
            "no card is below local (grew is above) -> stop signal fires"
        );
    }

    #[tokio::test]
    async fn smart_sync_does_not_drop_a_copy_when_a_card_straddles_the_stop_page() {
        // End-to-end regression for the reported bug (an import totalling N copies dropped to
        // N-1 on the next smart sync). Card `a` is owned as 2 regular copies the provider
        // reports as two SEPARATE regular rows (e.g. two conditions) whose `updatedAt`s put
        // them on non-adjacent pages, with a fully-in-sync `b` page in between. Driving the
        // same page loop `fetch_smart` runs, the middle page must not stop the fetch and
        // strand a's second row — which reconcile_smart would then overwrite a down to 1.
        let db = migrated_memory_db().await;
        let user_id = insert_user(&db, "straddle@test.example").await;
        let a = insert_card(&db, "ext-a").await;
        let b = insert_card(&db, "ext-b").await;
        insert_holding(&db, user_id, a, 2, 0).await; // owned: 2 regular
        insert_holding(&db, user_id, b, 1, 0).await; // owned: 1 regular

        let local = HashMap::from([("ext-a".to_string(), (2, 0)), ("ext-b".to_string(), (1, 0))]);
        let remap: HashMap<String, String> = HashMap::new();

        let pages: Vec<Vec<(String, bool, i32)>> = vec![
            vec![("ext-a".to_string(), false, 1)], // page 1: one of a's two rows
            vec![("ext-b".to_string(), false, 1)], // page 2: b in sync, a absent
            vec![("ext-a".to_string(), false, 1)], // page 3: a's second row
        ];
        let mut running: HashMap<String, (i64, i64)> = HashMap::new();
        let mut holdings: Vec<FetchedHolding> = Vec::new();
        let mut stopped_early = false;
        for page in pages {
            if smart_absorb_page(&mut running, &mut holdings, &local, &remap, page) {
                stopped_early = true;
                break;
            }
        }

        // The middle page must not have stopped the fetch, so both of a's rows were absorbed.
        assert!(
            stopped_early,
            "still stops early once a is fully aggregated on page 3"
        );
        assert_eq!(
            running["ext-a"],
            (2, 0),
            "a fully aggregated, not stranded at 1"
        );

        reconcile_smart(
            &db,
            user_id,
            crate::scryfall::GAME,
            Provider::Archidekt,
            holdings,
            stopped_early,
        )
        .await
        .expect("reconcile smart");

        assert_eq!(
            owned_counts(&db, user_id, a).await,
            Some((2, 0)),
            "no copy lost from the straddling card"
        );
        assert_eq!(
            owned_counts(&db, user_id, b).await,
            Some((1, 0)),
            "the in-sync card is unchanged"
        );
    }
}
