use super::*;
use crate::db::Dialect;
use crate::error::AppError;
use sea_orm::sea_query::{Alias, Expr, PostgresQueryBuilder, Query, SqliteQueryBuilder};

/// Render a parsed query's WHERE clause to inlined SQLite SQL for assertions.
fn sql(input: &str) -> String {
    let cond = parse(input, Dialect::Sqlite).expect("query should parse");
    Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("cards"))
        .cond_where(cond)
        .to_string(SqliteQueryBuilder)
}

/// Render a parsed query's WHERE clause to inlined **Postgres** SQL for assertions,
/// so the per-backend divergences (`~*`, `->>`, `IS TRUE`, `STRPOS`, `$N`
/// placeholders, POSIX integer guard) can be pinned.
fn pg_sql(input: &str) -> String {
    let cond = parse(input, Dialect::Postgres).expect("query should parse");
    Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("cards"))
        .cond_where(cond)
        .to_string(PostgresQueryBuilder)
}

#[test]
fn empty_query_matches_everything() {
    // Empty/whitespace queries impose no column predicate (a trivial match-all).
    for q in ["", "   "] {
        let s = sql(q);
        assert!(!s.contains("LIKE"), "{q:?} -> {s}");
        assert!(!s.contains("IFNULL"), "{q:?} -> {s}");
    }
}

#[test]
fn bare_word_is_name_substring() {
    assert!(sql("bolt").contains("LIKE '%bolt%'"));
}

#[test]
fn multiple_words_and_together() {
    let s = sql("lightning bolt");
    assert!(s.contains("LIKE '%lightning%'"));
    assert!(s.contains("LIKE '%bolt%'"));
    assert!(s.contains("AND"));
}

#[test]
fn quoted_phrase_is_one_term() {
    assert!(sql("\"lightning bolt\"").contains("LIKE '%lightning bolt%'"));
}

#[test]
fn like_wildcards_are_escaped() {
    assert!(sql("50%").contains("LIKE '%50\\%%'"));
    assert!(sql("a_b").contains("LIKE '%a\\_b%'"));
}

#[test]
fn sql_injection_in_name_search_is_escaped_not_interpolated() {
    // A quoted phrase keeps the whole injection payload as one name-substring
    // value. The renderer must double the single quote *inside* the bound
    // literal, so the inlined SQL contains the escaped pattern and never the
    // raw one that would close the string and start a second statement. We
    // assert the exact literal (not just the presence of `''`, which the
    // `COALESCE(col, '')` wrappers emit anyway) so the test actually fails if the
    // value were interpolated unescaped.
    let s = sql(r#""'; DROP TABLE cards;--""#);
    // The bound value is lowercased (LOWER-both case folding), so the payload is
    // lowercase inside the literal — but still escaped, never interpolated raw.
    assert!(
        s.contains("'%''; drop table cards;--%'"),
        "the value's quote must be doubled inside the literal: {s}"
    );
    assert!(
        !s.contains("'%'; drop table cards;--%'"),
        "the raw, unescaped payload must never reach the SQL: {s}"
    );
}

#[test]
fn sql_injection_in_oracle_filter_is_escaped() {
    // Same guarantee for a quoted value inside a typed filter (oracle text).
    let s = sql(r#"o:"'; DROP TABLE cards;--""#);
    assert!(s.contains("LOWER(COALESCE(oracle_text, '')) LIKE"), "{s}");
    assert!(
        s.contains("'%''; drop table cards;--%'"),
        "the value's quote must be doubled inside the literal: {s}"
    );
    assert!(
        !s.contains("'%'; drop table cards;--%'"),
        "the raw, unescaped payload must never reach the SQL: {s}"
    );
}

#[test]
fn deeply_nested_parentheses_are_rejected() {
    // The parenthesis-depth cap guards the public, unauthenticated search route
    // against stack exhaustion. It fires before the token cap (MAX_DEPTH*2 + 1
    // tokens < MAX_TOKENS), so this is a distinct DoS bound that
    // `too_many_tokens_is_rejected` would not catch if it regressed.
    let q = format!(
        "{}a{}",
        "(".repeat(MAX_DEPTH + 2),
        ")".repeat(MAX_DEPTH + 2)
    );
    assert!(
        matches!(parse(&q, Dialect::Sqlite), Err(SearchError::TooComplex)),
        "deep nesting must be rejected as too complex: {:?}",
        parse(&q, Dialect::Sqlite)
    );
}

#[test]
fn search_error_maps_to_422_validation() {
    // Unparseable / unsupported queries surface as 422, never a 500.
    let err: AppError = SearchError::UnknownKey("foo".to_string()).into();
    assert!(matches!(err, AppError::Validation(_)));
}

#[test]
fn malformed_and_unknown_filters_are_rejected() {
    assert!(
        parse("(t:creature", Dialect::Sqlite).is_err(),
        "unbalanced parenthesis"
    );
    assert!(
        parse("boguskey:value", Dialect::Sqlite).is_err(),
        "unknown filter key"
    );
}

#[test]
fn exact_name_has_no_surrounding_wildcards() {
    let s = sql("!\"Lightning Bolt\"");
    // Exact match is LOWER-both, so the bound literal is lowercased and unwrapped.
    assert!(s.contains("LIKE 'lightning bolt'"));
    assert!(!s.contains("%lightning bolt%"));
}

#[test]
fn type_and_oracle_substring() {
    assert!(sql("t:creature").contains("LOWER(COALESCE(type_line, '')) LIKE '%creature%'"));
    assert!(sql("o:flying").contains("LOWER(COALESCE(oracle_text, '')) LIKE '%flying%'"));
}

#[test]
fn color_at_least_uses_has() {
    let s = sql("c:r");
    assert!(s.contains("|| COALESCE(colors, '') ||"));
    assert!(s.contains("LIKE '%,R,%'"));
}

#[test]
fn color_exact_has_and_lacks() {
    let s = sql("c=rw");
    assert!(s.contains("LIKE '%,R,%'"));
    assert!(s.contains("LIKE '%,W,%'"));
    assert!(s.contains("NOT LIKE '%,U,%'"));
    assert!(s.contains("NOT LIKE '%,B,%'"));
    assert!(s.contains("NOT LIKE '%,G,%'"));
}

#[test]
fn color_subset_only_lacks_complement() {
    let s = sql("c<=uw");
    assert!(s.contains("NOT LIKE '%,B,%'"));
    assert!(s.contains("NOT LIKE '%,R,%'"));
    assert!(s.contains("NOT LIKE '%,G,%'"));
    assert!(!s.contains(" LIKE '%,W,%'")); // no positive has() for a subset query
}

#[test]
fn nickname_resolves_to_letters() {
    let s = sql("c>=esper");
    assert!(s.contains("LIKE '%,W,%'"));
    assert!(s.contains("LIKE '%,U,%'"));
    assert!(s.contains("LIKE '%,B,%'"));
}

#[test]
fn colorless_and_multicolor_tokens() {
    assert!(sql("c:c").contains("colors IS NULL"));
    assert!(sql("c!=c").contains("colors IS NOT NULL"));
    assert!(sql("c:m").contains("COALESCE(colors, '') LIKE '%,%'"));
}

#[test]
fn color_count() {
    assert!(sql("c=3").contains("REPLACE(colors, ',', '')"));
}

#[test]
fn identity_uses_its_column() {
    assert!(sql("id:r").contains("COALESCE(color_identity, '') ||"));
    assert!(sql("id<=wu").contains("COALESCE(color_identity, '') ||"));
}

#[test]
fn mana_value_numeric() {
    assert!(sql("mv>=3").contains("cmc >= 3"));
    assert!(sql("cmc:3").contains("cmc = 3"));
    assert!(sql("mv:even").contains("% 2 = 0"));
}

#[test]
fn power_text_and_range() {
    assert!(sql("pow=*").contains("COALESCE(power, '') = '*'"));
    let r = sql("pow>=5");
    // The integer-string guard (SQLite GLOB) lives inside a CASE (CAST in the THEN so a
    // non-numeric value yields NULL instead of erroring) AND is re-ANDed as a total
    // outer guard, so the leaf is 0/1 (never NULL) and `-pow>=5` still matches
    // non-numeric-power rows instead of dropping them.
    assert!(r.contains("GLOB '[0-9]*'"), "{r}");
    assert!(r.contains(") AND (CASE WHEN"), "{r}");
    assert!(r.contains("CAST(power AS REAL) ELSE NULL END >= 5)"), "{r}");
}

#[test]
fn power_cross_column() {
    let s = sql("pow>tou");
    // Both columns' integer-string guards are re-ANDed as total outer guards ahead of
    // the two guarded CASEs, so the leaf stays total and `-pow>tou` negates cleanly.
    assert!(s.contains(") AND (toughness IS NOT NULL"), "{s}");
    assert!(
        s.contains("CAST(power AS REAL) ELSE NULL END > CASE WHEN toughness"),
        "{s}"
    );
    assert!(s.contains("CAST(toughness AS REAL) ELSE NULL END"), "{s}");
}

#[test]
fn prices_cast() {
    // The decimal guard is re-ANDed outside the CASE so the leaf stays total (a missing
    // price fails the guard → 0/1, never NULL) and `-usd<1` still matches unpriced cards.
    let u = sql("usd<1");
    assert!(
        u.contains("(price_usd IS NOT NULL AND price_usd <> '') AND (CASE WHEN"),
        "{u}"
    );
    assert!(
        u.contains("CAST(price_usd AS REAL) ELSE NULL END < 1)"),
        "{u}"
    );
    assert!(sql("tix<=0.25").contains("CAST(price_tix AS REAL) ELSE NULL END <= 0.25"));
}

#[test]
fn year_and_date() {
    assert!(sql("year<=2010").contains("CAST(substr(released_at, 1, 4) AS INTEGER) <= 2010"));
    assert!(sql("date>=2015-01-01").contains("released_at >= '2015-01-01'"));
    assert!(sql("date<2018").contains("released_at < '2018-01-01'"));
    assert!(sql("date=2019").contains("released_at LIKE '2019-%'"));
}

#[test]
fn rarity_eq_and_ordered() {
    assert!(sql("r:mythic").contains("COALESCE(rarity, '') = 'mythic'"));
    let s = sql("r>=rare");
    assert!(s.contains("IN ('rare', 'special', 'mythic', 'bonus')"));
    assert!(sql("r<uncommon").contains("IN ('common')"));
}

#[test]
fn set_and_collector_number() {
    assert!(sql("e:DOM").contains("set_code = 'dom'"));
    assert!(sql("cn:12a").contains("lower(collector_number) = '12a'"));
    assert!(sql("cn>=250").contains("collector_number_int >= 250"));
}

#[test]
fn set_type_filter() {
    let s = sql("st:expansion");
    // Resolved via a game-scoped subquery on the set code, not a cards column.
    assert!(s.contains("set_code IN (SELECT code FROM card_sets"), "{s}");
    assert!(s.contains("game = 'mtg'"), "{s}");
    assert!(
        s.contains("LOWER(COALESCE(set_type, '')) = 'expansion'"),
        "{s}"
    );
    // != negates the membership test.
    assert!(sql("st!=promo").contains("set_code NOT IN (SELECT code FROM card_sets"));
    // settype is an alias; Scryfall's st: aliases map to the stored set_type.
    assert!(sql("settype:unset").contains("= 'funny'"));
    assert!(sql("st:boxset").contains("= 'box'"));
}

#[test]
fn set_type_rejects_range_operators() {
    assert!(matches!(
        parse("st>core", Dialect::Sqlite),
        Err(SearchError::UnsupportedOperator { .. })
    ));
}

/// Execute the compiled conditions against a real in-memory SQLite so the
/// cross-table `st:` subquery and the front-face `is:spell` logic are proven
/// on rows, not just asserted against rendered SQL.
#[tokio::test]
async fn set_type_and_spell_run_over_sqlite() {
    use crate::entities::prelude::Card;
    use crate::entities::{card, card_set};
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: sea_orm::prelude::DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();

    for (code, st) in [("eaa", "expansion"), ("cmm", "commander")] {
        card_set::ActiveModel {
            game: Set("mtg".to_owned()),
            code: Set(code.to_owned()),
            name: Set(format!("Set {code}")),
            set_type: Set(Some(st.to_owned())),
            card_count: Set(0),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    // (set_code, name, type_line, layout). Kazandu Mammoth is a spell//land
    // modal DFC: castable front, Land back — the case the fix protects.
    let cards = [
        ("eaa", "Grizzly Bears", "Creature — Bear", "normal"),
        (
            "eaa",
            "Kazandu Mammoth",
            "Creature — Elephant // Land",
            "modal_dfc",
        ),
        ("eaa", "Forest", "Basic Land — Forest", "normal"),
        ("eaa", "Bear Token", "Creature — Bear", "token"),
        ("eaa", "Lightning Bolt", "Instant", "normal"),
        ("cmm", "Command Tower", "Land", "normal"),
    ];
    for (i, (sc, name, tl, layout)) in cards.iter().enumerate() {
        card::ActiveModel {
            game: Set("mtg".to_owned()),
            external_id: Set(format!("ext-{i}")),
            name: Set((*name).to_owned()),
            set_code: Set((*sc).to_owned()),
            set_name: Set(format!("Set {sc}")),
            collector_number: Set(i.to_string()),
            lang: Set("en".to_owned()),
            type_line: Set(Some((*tl).to_owned())),
            layout: Set(Some((*layout).to_owned())),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    async fn names(db: &DatabaseConnection, q: &str) -> Vec<String> {
        let mut v = Card::find()
            .filter(parse(q, Dialect::Sqlite).expect("parses"))
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        v.sort();
        v
    }

    let eaa = vec![
        "Bear Token",
        "Forest",
        "Grizzly Bears",
        "Kazandu Mammoth",
        "Lightning Bolt",
    ];
    // st: resolves the set's set_type via the card_sets subquery; negation via
    // NOT IN stays exact (set_code is non-null).
    assert_eq!(names(&db, "st:expansion").await, eaa);
    assert_eq!(names(&db, "-st:commander").await, eaa);
    assert_eq!(names(&db, "st:commander").await, vec!["Command Tower"]);
    assert!(names(&db, "st:funny").await.is_empty()); // unknown type -> no rows

    // is:spell keeps the spell//land DFC (front is a creature) and the plain
    // creature/instant; drops the basic land, the Command Tower land, and the
    // token printing.
    assert_eq!(
        names(&db, "is:spell").await,
        vec!["Grizzly Bears", "Kazandu Mammoth", "Lightning Bolt"]
    );
    // is:permanent: everything except the instant.
    assert_eq!(
        names(&db, "is:permanent").await,
        vec![
            "Bear Token",
            "Command Tower",
            "Forest",
            "Grizzly Bears",
            "Kazandu Mammoth",
        ]
    );
}

#[test]
fn lang_any_is_no_filter() {
    assert!(!sql("lang:any").contains("lang ="));
    assert!(sql("lang:japanese").contains("lang = 'ja'"));
}

#[test]
fn is_predicates() {
    assert!(sql("is:split").contains("COALESCE(layout, '') = 'split'"));
    assert!(sql("is:dfc").contains("IN ('transform', 'modal_dfc', 'meld', 'reversible_card')"));
    assert!(sql("is:colorless").contains("colors IS NULL"));
    assert!(sql("is:phyrexian").contains("LIKE '%/P}%'"));
}

#[test]
fn type_derived_is_predicates() {
    let perm = sql("is:permanent");
    assert!(
        perm.contains("LOWER(type_line) LIKE '%creature%'"),
        "{perm}"
    );
    assert!(perm.contains("NOT LIKE '%instant%'"), "{perm}");
    assert!(perm.contains("NOT LIKE '%sorcery%'"), "{perm}");
    // is:spell tests only the FRONT face's type for land-ness, so a spell//land
    // modal DFC (e.g. "Creature — Elephant // Land") is kept, not dropped.
    let spell = sql("is:spell");
    assert!(spell.contains("INSTR(type_line, ' // ')"), "{spell}");
    assert!(spell.contains("NOT LIKE '%land%'"), "{spell}");
    assert!(sql("is:vanilla").contains("oracle_text IS NULL OR oracle_text = ''"));
    // Predicates are total, so not: negates them.
    assert!(sql("not:permanent").contains("NOT"));
    // Still rejects unknown is: values.
    assert!(matches!(
        parse("is:bear", Dialect::Sqlite),
        Err(SearchError::UnsupportedKey(_))
    ));
}

#[test]
fn negation_is_not_wrapped() {
    assert!(sql("-t:land").contains("NOT"));
    assert!(sql("not:transform").contains("NOT"));
}

#[test]
fn boolean_precedence() {
    // a or b c  ==  a OR (b AND c)
    let s = sql("a or b c");
    assert!(s.contains("OR"));
    assert!(s.contains("AND"));
}

#[test]
fn grouping_with_parens() {
    let s = sql("(c:r or c:u) t:instant");
    assert!(s.contains("OR"));
    assert!(s.contains("LOWER(COALESCE(type_line, '')) LIKE '%instant%'"));
}

#[test]
fn case_insensitive_keyword_and_value() {
    assert_eq!(sql("C:R"), sql("c:r"));
}

fn err(input: &str) -> SearchError {
    parse(input, Dialect::Sqlite).expect_err("should be an error")
}

#[test]
fn error_cases() {
    assert!(matches!(err("foo:bar"), SearchError::UnknownKey(_)));
    // Deferred filters (Tagger tags #140, cube #141, Phase-5 aggregates) still 422.
    assert!(matches!(
        err("cube:vintage"),
        SearchError::UnsupportedKey(_)
    ));
    assert!(matches!(
        err("otag:removal"),
        SearchError::UnsupportedKey(_)
    ));
    assert!(matches!(err("block:rtr"), SearchError::UnsupportedKey(_)));
    assert!(matches!(
        err("set>dom"),
        SearchError::UnsupportedOperator { .. }
    ));
    assert!(matches!(
        err("mana<=2"),
        SearchError::UnsupportedOperator { .. }
    ));
    assert!(matches!(err("t:"), SearchError::MissingValue { .. }));
    assert!(matches!(err("cmc>=x"), SearchError::InvalidValue { .. }));
    assert!(matches!(err("c:x"), SearchError::InvalidValue { .. }));
    assert!(matches!(err("cn>=12a"), SearchError::InvalidValue { .. }));
    assert!(matches!(
        err("r:legendary"),
        SearchError::InvalidValue { .. }
    ));
    assert!(matches!(err(">=3"), SearchError::MissingKey));
    assert!(matches!(err("()"), SearchError::EmptyGroup));
    assert!(matches!(err("(c:r or c:u"), SearchError::UnbalancedParen));
    assert!(matches!(err("a)"), SearchError::UnexpectedToken(_)));
    assert!(matches!(err("a or"), SearchError::UnexpectedEof));
    assert!(matches!(err("\"abc"), SearchError::UnterminatedString));
}

#[test]
fn too_many_tokens_is_rejected() {
    let big = "a ".repeat(MAX_TOKENS + 10);
    assert!(matches!(
        parse(&big, Dialect::Sqlite),
        Err(SearchError::TooComplex)
    ));
}

#[test]
fn mana_containment_with_multiplicity() {
    let s = sql("m:2WW");
    assert!(s.contains("REPLACE(COALESCE(mana_cost, ''), '{2}', '')"));
    assert!(s.contains("REPLACE(COALESCE(mana_cost, ''), '{W}', '')"));
    // {W} appears twice -> threshold 2 * len('{W}') = 6
    assert!(s.contains(">= 6"));
}

#[test]
fn mana_hybrid_normalized() {
    assert!(sql("m:{u/w}").contains("{W/U}"));
}

#[test]
fn mana_exact_is_order_independent_multiset() {
    let s = sql("mana=2WW");
    // Exact = containment (per symbol) + equal total symbol count (3 symbols).
    assert!(s.contains("'}', ''))) = 3"), "{s}");
    assert!(s.contains("'{W}', ''))) >= 6"), "{s}");
    // Not the old order-sensitive string-equality form.
    assert!(!s.contains("= '{2}{W}{W}'"), "{s}");
}

#[test]
fn cmc_parity_rejects_relational_operator() {
    assert!(matches!(
        parse("mv>even", Dialect::Sqlite),
        Err(SearchError::UnsupportedOperator { .. })
    ));
    assert!(sql("mv:even").contains("% 2 = 0"));
}

#[test]
fn oversized_query_is_rejected() {
    let big = "a".repeat(MAX_QUERY_BYTES + 1);
    assert!(matches!(
        parse(&big, Dialect::Sqlite),
        Err(SearchError::TooComplex)
    ));
}

#[test]
fn too_many_mana_symbols_rejected() {
    let q = format!("m:{}", "{W}".repeat(MAX_MANA_SYMBOLS + 1));
    assert!(matches!(
        parse(&q, Dialect::Sqlite),
        Err(SearchError::InvalidValue { .. })
    ));
}

// ----- Column-backed filters (search parity, Phase 2) -----

#[test]
fn keyword_filter_is_comma_delimited_membership() {
    let s = sql("kw:flying");
    assert!(s.contains("keywords"), "{s}");
    assert!(s.contains("'%,flying,%'"), "{s}");
}

#[test]
fn legality_uses_json_extract() {
    let s = sql("f:modern");
    assert!(s.contains("json_extract"), "{s}");
    assert!(s.contains("'$.modern'"), "{s}");
    assert!(s.contains("'legal'") && s.contains("'restricted'"), "{s}");
    assert!(sql("banned:legacy").contains("'banned'"));
    assert!(sql("restricted:vintage").contains("'restricted'"));
}

#[test]
fn finish_and_flag_is_subjects_compile() {
    // foil must not match nonfoil (comma-delimited membership).
    assert!(sql("is:foil").contains("finishes"));
    assert!(sql("is:foil").contains("'%,foil,%'"));
    assert!(sql("is:reprint").contains("reprint IS TRUE"));
    assert!(sql("-is:reprint").contains("NOT"));
    assert!(sql("is:promo").contains("promo IS TRUE"));
    assert!(sql("is:buyabox").contains("promo_types"));
}

#[test]
fn print_detail_filters_compile() {
    assert!(sql("border:borderless").contains("border_color"));
    assert!(sql("stamp:acorn").contains("security_stamp"));
    assert!(sql("wm:izzet").contains("watermark"));
    assert!(sql("a:\"rebecca guay\"").contains("artist"));
    assert!(sql("ft:draw").contains("flavor_text"));
    assert!(sql("has:flavor").contains("flavor_text IS NOT NULL"));
    // frame matches the frame edition OR a frame effect.
    let f = sql("frame:showcase");
    assert!(f.contains("frame_effects"), "{f}");
    assert!(sql("produces:wu").contains("produced_mana"));
    assert!(sql("artists>1").contains("artist_ids"));
}

#[test]
fn deferred_filters_still_422() {
    for q in [
        "cube:vintage",
        "otag:removal",
        "atag:squirrel",
        "devotion:2",
    ] {
        assert!(
            matches!(
                parse(q, Dialect::Sqlite),
                Err(SearchError::UnsupportedKey(_))
            ),
            "{q} should still be unsupported"
        );
    }
}

#[test]
fn result_shaping_directives_are_extracted() {
    // order:/direction:/unique: are pulled out of the filter and don't 422.
    let q = parse_query(
        "c:r order:edhrec direction:desc unique:cards",
        Dialect::Sqlite,
    )
    .unwrap();
    assert_eq!(q.order, Some(SortKey::Edhrec));
    assert_eq!(q.direction, Some(Direction::Desc));
    assert_eq!(q.unique, Some(UniqueMode::Cards));
    // The remaining filter still applies.
    let rendered = Query::select()
        .expr(Expr::val(1))
        .from(Alias::new("cards"))
        .cond_where(q.condition)
        .to_string(SqliteQueryBuilder);
    assert!(rendered.contains("colors"), "{rendered}");
    // Last-one-wins on duplicates.
    assert_eq!(
        parse_query("order:name order:cmc", Dialect::Sqlite)
            .unwrap()
            .order,
        Some(SortKey::Cmc)
    );
    // A negated directive is rejected; an unknown value 422s.
    assert!(matches!(
        parse_query("-order:cmc", Dialect::Sqlite),
        Err(SearchError::InvalidValue { .. })
    ));
    assert!(matches!(
        parse_query("order:bogus", Dialect::Sqlite),
        Err(SearchError::InvalidValue { .. })
    ));
    // parse() (condition-only) simply drops the directives without erroring.
    assert!(parse("order:cmc", Dialect::Sqlite).is_ok());
}

/// Run the new column-backed filters against a real in-memory SQLite so
/// `json_extract` (legalities) and comma-membership (keywords / finishes /
/// promo_types) are proven on rows, not just asserted against rendered SQL.
#[tokio::test]
async fn column_backed_filters_run_over_sqlite() {
    use crate::entities::card;
    use crate::entities::prelude::Card;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: sea_orm::prelude::DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();

    // name, keywords, finishes, promo_types, legalities json, reprint, border, artist
    let rows = [
        (
            "Flyer",
            Some("Flying"),
            Some("nonfoil,foil"),
            None,
            Some(r#"{"modern":"legal","legacy":"banned"}"#),
            Some(true),
            Some("black"),
            Some("Rebecca Guay"),
        ),
        (
            "Grounder",
            Some("Trample"),
            Some("nonfoil"),
            None,
            Some(r#"{"modern":"legal","legacy":"legal"}"#),
            Some(false),
            Some("borderless"),
            Some("Someone Else"),
        ),
        (
            "Promoish",
            None,
            Some("foil,etched"),
            Some("buyabox"),
            Some(r#"{"modern":"banned"}"#),
            Some(false),
            Some("black"),
            None,
        ),
    ];
    for (i, &(name, kw, fin, promo, leg, reprint, border, artist)) in rows.iter().enumerate() {
        card::ActiveModel {
            game: Set("mtg".to_owned()),
            external_id: Set(format!("ext-{i}")),
            name: Set(name.to_owned()),
            set_code: Set("tst".to_owned()),
            set_name: Set("TST".to_owned()),
            collector_number: Set(i.to_string()),
            lang: Set("en".to_owned()),
            keywords: Set(kw.map(str::to_owned)),
            finishes: Set(fin.map(str::to_owned)),
            promo_types: Set(promo.map(str::to_owned)),
            legalities: Set(leg.map(str::to_owned)),
            reprint: Set(reprint),
            border_color: Set(border.map(str::to_owned)),
            artist: Set(artist.map(str::to_owned)),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    async fn names(db: &DatabaseConnection, q: &str) -> Vec<String> {
        let mut v = Card::find()
            .filter(parse(q, Dialect::Sqlite).expect("parses"))
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        v.sort();
        v
    }

    assert_eq!(names(&db, "kw:flying").await, vec!["Flyer"]);
    assert_eq!(names(&db, "kw:trample").await, vec!["Grounder"]);
    // Legality via json_extract: banned is excluded from f:, surfaced by banned:.
    assert_eq!(names(&db, "f:legacy").await, vec!["Grounder"]);
    assert_eq!(names(&db, "banned:legacy").await, vec!["Flyer"]);
    assert_eq!(names(&db, "f:modern").await, vec!["Flyer", "Grounder"]);
    // Finish membership: foil matches Flyer & Promoish, not the nonfoil-only card.
    assert_eq!(names(&db, "is:foil").await, vec!["Flyer", "Promoish"]);
    assert_eq!(names(&db, "is:etched").await, vec!["Promoish"]);
    assert_eq!(names(&db, "is:reprint").await, vec!["Flyer"]);
    assert_eq!(names(&db, "is:buyabox").await, vec!["Promoish"]);
    assert_eq!(names(&db, "border:borderless").await, vec!["Grounder"]);
    assert_eq!(names(&db, "a:rebecca").await, vec!["Flyer"]);
    // Negation stays exact/total (nonfoil-only card is the only non-foil).
    assert_eq!(names(&db, "-is:foil").await, vec!["Grounder"]);
}

/// Confirms the sqlx `regexp` feature registers a REGEXP function on SeaORM's
/// SQLite connections (so the o:/…/ regex filters can rely on it) and that the
/// bundled SQLite has the JSON1 `json_extract` used by the legality filters.
#[tokio::test]
async fn sqlite_has_regexp_and_json_functions() {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let db = crate::test_support::migrated_memory_db().await;
    let row = db
        .query_one(Statement::from_string(
            DatabaseBackend::Sqlite,
            "SELECT ('abcd' REGEXP 'b.d') AS rx, \
             json_extract('{\"modern\":\"legal\"}', '$.modern') AS jx",
        ))
        .await
        .unwrap()
        .expect("one row");
    let rx: i32 = row.try_get("", "rx").unwrap();
    let jx: String = row.try_get("", "jx").unwrap();
    assert_eq!(rx, 1, "REGEXP function must be registered");
    assert_eq!(jx, "legal", "json_extract must be available");
}

// ----- Structural features (search parity, Phase 3) -----

#[test]
fn regex_literal_compiles_to_regexp() {
    assert!(sql("o:/counter target/").contains("REGEXP"));
    // Case-insensitive prefix is applied to the bound pattern (spaces allowed).
    assert!(
        sql("o:/counter target/").contains("(?i)counter target"),
        "{}",
        sql("o:/counter target/")
    );
    // A bare /…/ is a name-field regex.
    assert!(sql("/^bolt/").contains("COALESCE(name, '') REGEXP"));
    // An escaped slash stays inside the pattern.
    assert!(sql(r"o:/a\/b/").contains(r"(?i)a\/b"));
}

#[test]
fn regex_errors() {
    assert!(matches!(err("o:/foo"), SearchError::UnterminatedRegex));
    assert!(matches!(err("o:/(/"), SearchError::InvalidValue { .. }));
}

#[test]
fn mana_relational_operators() {
    // Strict superset adds a total-symbol-count comparison.
    assert!(sql("m>{R}").contains("> 1"), "{}", sql("m>{R}"));
    assert!(sql("m!={R}").contains("NOT"));
    // Subset (`<`, `<=`) is still unsupported.
    assert!(matches!(
        err("m<=2"),
        SearchError::UnsupportedOperator { .. }
    ));
}

#[test]
fn color_name_words_and_commander_alias() {
    assert_eq!(sql("c:white"), sql("c:w"));
    assert_eq!(sql("id:green"), sql("id:g"));
    // commander:/cmdr: alias the colour-identity column.
    assert!(sql("commander:wu").contains("color_identity"));
}

#[test]
fn pt_defense_and_fulloracle() {
    let s = sql("pt>=6");
    assert!(
        s.contains("power") && s.contains("toughness") && s.contains('+'),
        "{s}"
    );
    assert!(sql("def>3").contains("defense"));
    assert_eq!(sql("fulloracle:draw"), sql("o:draw"));
}

/// Prove regex actually filters rows through the registered REGEXP function.
#[tokio::test]
async fn regex_filter_runs_over_sqlite() {
    use crate::entities::card;
    use crate::entities::prelude::Card;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: sea_orm::prelude::DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    for (i, (name, oracle)) in [
        ("Tapper", "{T}: Add {G}."),
        ("Drawer", "Draw a card."),
        ("Bolt", "Deal 3 damage."),
    ]
    .iter()
    .enumerate()
    {
        card::ActiveModel {
            game: Set("mtg".into()),
            external_id: Set(format!("ext-{i}")),
            name: Set((*name).into()),
            set_code: Set("tst".into()),
            set_name: Set("TST".into()),
            collector_number: Set(i.to_string()),
            lang: Set("en".into()),
            oracle_text: Set(Some((*oracle).into())),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    async fn names(db: &DatabaseConnection, q: &str) -> Vec<String> {
        let mut v = Card::find()
            .filter(parse(q, Dialect::Sqlite).expect("parses"))
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        v.sort();
        v
    }

    // Regex over oracle text: anchored + case-insensitive.
    assert_eq!(names(&db, r"o:/^\{T\}/").await, vec!["Tapper"]);
    assert_eq!(names(&db, "o:/draw a card/").await, vec!["Drawer"]);
    // A bare /…/ regexes the name.
    assert_eq!(names(&db, "/bolt/").await, vec!["Bolt"]);
}

// ----- Sibling-print aggregates (search parity, Phase 5) -----

#[test]
fn print_count_filters_compile() {
    let s = sql("prints>=2");
    assert!(s.contains("SELECT COUNT(*) FROM cards c2"), "{s}");
    assert!(s.contains("c2.oracle_id = cards.oracle_id"), "{s}");
    assert!(sql("sets>1").contains("COUNT(DISTINCT c2.set_code)"));
}

/// prints/sets count a card's printings via the oracle_id-sibling subquery.
#[tokio::test]
async fn print_count_filters_run_over_sqlite() {
    use crate::entities::card;
    use crate::entities::prelude::Card;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: sea_orm::prelude::DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();
    // o1 is reprinted across two sets; o2 has a single printing; one card has no
    // oracle id (its own sole sibling).
    for (i, (name, set, oracle)) in [
        ("Rep A", "s1", Some("o1")),
        ("Rep B", "s2", Some("o1")),
        ("Solo", "s1", Some("o2")),
        ("Nul", "s1", None),
    ]
    .iter()
    .enumerate()
    {
        card::ActiveModel {
            game: Set("mtg".into()),
            external_id: Set(format!("ext-{i}")),
            name: Set((*name).into()),
            set_code: Set((*set).into()),
            set_name: Set("S".into()),
            collector_number: Set(i.to_string()),
            lang: Set("en".into()),
            oracle_id: Set(oracle.map(str::to_owned)),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    async fn names(db: &DatabaseConnection, q: &str) -> Vec<String> {
        let mut v = Card::find()
            .filter(parse(q, Dialect::Sqlite).expect("parses"))
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        v.sort();
        v
    }

    assert_eq!(names(&db, "prints>=2").await, vec!["Rep A", "Rep B"]);
    assert_eq!(names(&db, "prints=1").await, vec!["Nul", "Solo"]);
    assert_eq!(names(&db, "sets>1").await, vec!["Rep A", "Rep B"]);
    // Negation stays exact/total.
    assert_eq!(names(&db, "-prints>1").await, vec!["Nul", "Solo"]);
}

/// Regression (F1): a negated numeric-stat / price range must stay TOTAL, so a card
/// with a non-numeric stat (`*`, `X`) or no price is INCLUDED by the negation rather
/// than dropped. A leaf that only lived inside a `CASE … ELSE NULL` went NULL for
/// those rows, and `NOT NULL` is NULL (not true), silently excluding them — this test
/// exists because no execution-level negation test caught that.
#[tokio::test]
async fn negation_over_non_numeric_and_unpriced_stays_total_over_sqlite() {
    use crate::entities::card;
    use crate::entities::prelude::Card;
    use sea_orm::{ActiveModelTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

    let db = crate::test_support::migrated_memory_db().await;
    let ts: sea_orm::prelude::DateTimeUtc = "2024-01-01T00:00:00Z".parse().unwrap();

    // NonNumeric: power/toughness are `*` (non-numeric) and it has no USD price.
    // Numeric: plain-integer power/toughness and a sub-$1 price — the row the ranges
    // actually match, so each negation must EXCLUDE it and keep only NonNumeric.
    for (i, name, power, toughness, usd) in [
        (0, "NonNumeric", "*", "*", None),
        (1, "Numeric", "6", "6", Some("0.50")),
    ] {
        card::ActiveModel {
            game: Set("mtg".into()),
            external_id: Set(format!("ext-{i}")),
            name: Set(name.into()),
            set_code: Set("tst".into()),
            set_name: Set("TST".into()),
            collector_number: Set(i.to_string()),
            lang: Set("en".into()),
            power: Set(Some(power.into())),
            toughness: Set(Some(toughness.into())),
            price_usd: Set(usd.map(str::to_owned)),
            digital: Set(false),
            created_at: Set(ts),
            updated_at: Set(ts),
            ..Default::default()
        }
        .insert(&db)
        .await
        .unwrap();
    }

    async fn names(db: &DatabaseConnection, q: &str) -> Vec<String> {
        let mut v = Card::find()
            .filter(parse(q, Dialect::Sqlite).expect("parses"))
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        v.sort();
        v
    }

    // Each negation matches the non-numeric-stat / unpriced card (leaf is total-false
    // there, so NOT → true) and excludes the numeric/priced one (leaf true → NOT → false).
    assert_eq!(names(&db, "-pow>=5").await, vec!["NonNumeric"]);
    assert_eq!(names(&db, "-pt>5").await, vec!["NonNumeric"]);
    assert_eq!(names(&db, "-usd<1").await, vec!["NonNumeric"]);
}

// ----- Postgres dialect: pin the per-backend divergent SQL -----

#[test]
fn placeholders_renumber_only_on_postgres() {
    // Postgres renumbers `?`→`$N` left-to-right; SQLite is untouched. A `?` inside a
    // single-quoted string literal is left alone.
    assert_eq!(Dialect::Postgres.placeholders("a ? b ? c"), "a $1 b $2 c");
    assert_eq!(Dialect::Sqlite.placeholders("a ? b ? c"), "a ? b ? c");
    assert_eq!(
        Dialect::Postgres.placeholders("x = '?' AND y = ?"),
        "x = '?' AND y = $1"
    );
}

#[test]
fn pg_name_substring_is_lower_coalesce_and_bound() {
    let s = pg_sql("bolt");
    // LOWER-both case fold; the value is bound (renumbered to a `$N`), never a bare `?`.
    assert!(s.contains("LOWER(COALESCE(name, '')) LIKE '%bolt%'"), "{s}");
    assert!(
        !s.contains("LIKE ?"),
        "placeholder must be renumbered on PG: {s}"
    );
}

#[test]
fn pg_regex_uses_ci_operator() {
    let s = pg_sql("o:/foo/");
    assert!(s.contains("~*"), "{s}");
    assert!(!s.contains("REGEXP"), "{s}");
}

#[test]
fn pg_legality_uses_jsonb_arrow_and_bare_key() {
    let pg = pg_sql("f:standard");
    assert!(pg.contains("::jsonb ->>"), "{pg}");
    // The bound json key is the bare format name on Postgres, not a SQLite JSONPath.
    assert!(pg.contains("'standard'"), "{pg}");
    assert!(!pg.contains("'$.standard'"), "{pg}");
    // SQLite still uses json_extract with the `$.fmt` JSONPath.
    let lite = sql("f:standard");
    assert!(
        lite.contains("json_extract(legalities, '$.standard')"),
        "{lite}"
    );
}

#[test]
fn pg_integer_guard_is_posix_regex_inside_case() {
    let s = pg_sql("pow>2");
    assert!(s.contains("~ '^[0-9]+$'"), "{s}");
    assert!(!s.contains("GLOB"), "{s}");
    // The POSIX guard is also re-ANDed outside the CASE (F1 totality) on Postgres.
    assert!(s.contains("~ '^[0-9]+$') AND (CASE WHEN"), "{s}");
    assert!(s.contains("CAST(power AS REAL) ELSE NULL END > 2"), "{s}");
}

#[test]
fn pg_prices_cast_has_decimal_guard() {
    // Postgres's strict CAST hard-errors on a non-decimal string, so the price CAST is
    // guarded by a decimal-shape POSIX regex, re-ANDed outside the CASE (F1 totality).
    let s = pg_sql("usd<1");
    assert!(s.contains(r"~ '^[0-9]+(\.[0-9]+)?$'"), "{s}");
    assert!(
        s.contains("CAST(price_usd AS REAL) ELSE NULL END < 1"),
        "{s}"
    );
}

#[test]
fn pg_spell_uses_strpos() {
    let s = pg_sql("is:spell");
    assert!(s.contains("STRPOS("), "{s}");
    assert!(!s.contains("INSTR("), "{s}");
}

#[test]
fn pg_boolean_flag_is_true() {
    assert!(pg_sql("is:fullart").contains("full_art IS TRUE"));
}

#[test]
fn pg_multi_placeholder_fragment_binds_all_values_in_order() {
    // A multi-`?` clause per symbol: the renumber must preserve argument order, so
    // every value is bound (non-empty) and the placeholders are sequential `$N`.
    let cond = parse("m:2WW", Dialect::Postgres).expect("parses");
    // `Expr::cust("1")` is a raw fragment (not a bound value), so every bound value
    // in the built statement comes from the mana clauses only.
    let (rendered, values) = Query::select()
        .expr(Expr::cust("1"))
        .from(Alias::new("cards"))
        .cond_where(cond)
        .build(PostgresQueryBuilder);
    // {2}×1 and {W}×2 → two containment clauses, each binding (symbol, threshold).
    assert_eq!(values.0.len(), 4, "all four values bound: {rendered}");
    assert!(
        rendered.contains("$1") && rendered.contains("$4"),
        "{rendered}"
    );
    assert!(
        !rendered.contains(" ?"),
        "no bare `?` left on PG: {rendered}"
    );
}
