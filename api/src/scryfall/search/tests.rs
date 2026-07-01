use super::*;
use crate::error::AppError;
use sea_orm::sea_query::{Alias, Expr, Query, SqliteQueryBuilder};

    /// Render a parsed query's WHERE clause to inlined SQLite SQL for assertions.
    fn sql(input: &str) -> String {
        let cond = parse(input).expect("query should parse");
        Query::select()
            .expr(Expr::val(1))
            .from(Alias::new("cards"))
            .cond_where(cond)
            .to_string(SqliteQueryBuilder)
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
        // `IFNULL(col, '')` wrappers emit anyway) so the test actually fails if the
        // value were interpolated unescaped.
        let s = sql(r#""'; DROP TABLE cards;--""#);
        assert!(
            s.contains("'%''; DROP TABLE cards;--%'"),
            "the value's quote must be doubled inside the literal: {s}"
        );
        assert!(
            !s.contains("'%'; DROP TABLE cards;--%'"),
            "the raw, unescaped payload must never reach the SQL: {s}"
        );
    }

    #[test]
    fn sql_injection_in_oracle_filter_is_escaped() {
        // Same guarantee for a quoted value inside a typed filter (oracle text).
        let s = sql(r#"o:"'; DROP TABLE cards;--""#);
        assert!(s.contains("IFNULL(oracle_text, '') LIKE"), "{s}");
        assert!(
            s.contains("'%''; DROP TABLE cards;--%'"),
            "the value's quote must be doubled inside the literal: {s}"
        );
        assert!(
            !s.contains("'%'; DROP TABLE cards;--%'"),
            "the raw, unescaped payload must never reach the SQL: {s}"
        );
    }

    #[test]
    fn deeply_nested_parentheses_are_rejected() {
        // The parenthesis-depth cap guards the public, unauthenticated search route
        // against stack exhaustion. It fires before the token cap (MAX_DEPTH*2 + 1
        // tokens < MAX_TOKENS), so this is a distinct DoS bound that
        // `too_many_tokens_is_rejected` would not catch if it regressed.
        let q = format!("{}a{}", "(".repeat(MAX_DEPTH + 2), ")".repeat(MAX_DEPTH + 2));
        assert!(
            matches!(parse(&q), Err(SearchError::TooComplex)),
            "deep nesting must be rejected as too complex: {:?}",
            parse(&q)
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
        assert!(parse("(t:creature").is_err(), "unbalanced parenthesis");
        assert!(parse("boguskey:value").is_err(), "unknown filter key");
    }

    #[test]
    fn exact_name_has_no_surrounding_wildcards() {
        let s = sql("!\"Lightning Bolt\"");
        assert!(s.contains("LIKE 'Lightning Bolt'"));
        assert!(!s.contains("%Lightning Bolt%"));
    }

    #[test]
    fn type_and_oracle_substring() {
        assert!(sql("t:creature").contains("IFNULL(type_line, '') LIKE '%creature%'"));
        assert!(sql("o:flying").contains("IFNULL(oracle_text, '') LIKE '%flying%'"));
    }

    #[test]
    fn color_at_least_uses_has() {
        let s = sql("c:r");
        assert!(s.contains("|| IFNULL(colors, '') ||"));
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
        assert!(sql("c:m").contains("IFNULL(colors, '') LIKE '%,%'"));
    }

    #[test]
    fn color_count() {
        assert!(sql("c=3").contains("REPLACE(colors, ',', '')"));
    }

    #[test]
    fn identity_uses_its_column() {
        assert!(sql("id:r").contains("IFNULL(color_identity, '') ||"));
        assert!(sql("id<=wu").contains("IFNULL(color_identity, '') ||"));
    }

    #[test]
    fn mana_value_numeric() {
        assert!(sql("mv>=3").contains("cmc >= 3"));
        assert!(sql("cmc:3").contains("cmc = 3"));
        assert!(sql("mv:even").contains("% 2 = 0"));
    }

    #[test]
    fn power_text_and_range() {
        assert!(sql("pow=*").contains("IFNULL(power, '') = '*'"));
        let r = sql("pow>=5");
        assert!(r.contains("GLOB '[0-9]*'"));
        assert!(r.contains("CAST(power AS REAL) >= 5"));
    }

    #[test]
    fn power_cross_column() {
        let s = sql("pow>tou");
        assert!(s.contains("CAST(power AS REAL) > CAST(toughness AS REAL)"));
    }

    #[test]
    fn prices_cast() {
        assert!(sql("usd<1").contains("CAST(price_usd AS REAL) < 1"));
        assert!(sql("tix<=0.25").contains("CAST(price_tix AS REAL) <= 0.25"));
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
        assert!(sql("r:mythic").contains("IFNULL(rarity, '') = 'mythic'"));
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
            s.contains("LOWER(IFNULL(set_type, '')) = 'expansion'"),
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
            parse("st>core"),
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
                .filter(parse(q).expect("parses"))
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
        assert!(sql("is:split").contains("IFNULL(layout, '') = 'split'"));
        assert!(sql("is:dfc").contains("IN ('transform', 'modal_dfc', 'meld', 'reversible_card')"));
        assert!(sql("is:colorless").contains("colors IS NULL"));
        assert!(sql("is:phyrexian").contains("LIKE '%/P}%'"));
    }

    #[test]
    fn type_derived_is_predicates() {
        let perm = sql("is:permanent");
        assert!(perm.contains("type_line LIKE '%creature%'"), "{perm}");
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
            parse("is:bear"),
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
        assert!(s.contains("IFNULL(type_line, '') LIKE '%instant%'"));
    }

    #[test]
    fn case_insensitive_keyword_and_value() {
        assert_eq!(sql("C:R"), sql("c:r"));
    }

    fn err(input: &str) -> SearchError {
        parse(input).expect_err("should be an error")
    }

    #[test]
    fn error_cases() {
        assert!(matches!(err("foo:bar"), SearchError::UnknownKey(_)));
        // Deferred filters (Tagger tags #140, cube #141, Phase-5 aggregates) still 422.
        assert!(matches!(err("cube:vintage"), SearchError::UnsupportedKey(_)));
        assert!(matches!(err("otag:removal"), SearchError::UnsupportedKey(_)));
        assert!(matches!(err("prints>2"), SearchError::UnsupportedKey(_)));
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
        assert!(matches!(parse(&big), Err(SearchError::TooComplex)));
    }

    #[test]
    fn mana_containment_with_multiplicity() {
        let s = sql("m:2WW");
        assert!(s.contains("REPLACE(IFNULL(mana_cost, ''), '{2}', '')"));
        assert!(s.contains("REPLACE(IFNULL(mana_cost, ''), '{W}', '')"));
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
            parse("mv>even"),
            Err(SearchError::UnsupportedOperator { .. })
        ));
        assert!(sql("mv:even").contains("% 2 = 0"));
    }

    #[test]
    fn oversized_query_is_rejected() {
        let big = "a".repeat(MAX_QUERY_BYTES + 1);
        assert!(matches!(parse(&big), Err(SearchError::TooComplex)));
    }

    #[test]
    fn too_many_mana_symbols_rejected() {
        let q = format!("m:{}", "{W}".repeat(MAX_MANA_SYMBOLS + 1));
        assert!(matches!(parse(&q), Err(SearchError::InvalidValue { .. })));
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
        assert!(sql("is:reprint").contains("IFNULL(reprint, 0) = 1"));
        assert!(sql("-is:reprint").contains("NOT"));
        assert!(sql("is:promo").contains("IFNULL(promo, 0) = 1"));
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
        for q in ["cube:vintage", "otag:removal", "atag:squirrel", "prints>3", "order:cmc"] {
            assert!(
                matches!(parse(q), Err(SearchError::UnsupportedKey(_))),
                "{q} should still be unsupported"
            );
        }
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
                .filter(parse(q).expect("parses"))
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
