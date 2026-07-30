use sea_orm::entity::prelude::*;

/// SeaORM entity for the `life_session_players` table.
///
/// One row per **seat** in a tracked game: the player's display name, where
/// they sit (`position` + `rotation`), what they're on now (`life`), and how the game ended
/// for them (`result`). A seat has **no `user_id`** — it hangs off `session_id` — so every
/// seat-scoped route must load the parent session to prove ownership first, exactly like a
/// `deck_card`.
///
/// A seat may name **what was being played** in one of two mutually exclusive ways.
/// `deck_id` links it to one of the owner's `decks` rows, which is what makes the per-deck
/// win/loss record possible; `commander_card_id` instead names a `cards` row, for an opponent
/// whose deck you don't have but whose commander you know. The handler refuses both at once —
/// a seat carrying each would leave "what was played here" ambiguous. It is deliberately **orphan-tolerant** and
/// carries **no foreign key** (the same call `price_alerts.card_id` makes): a user may
/// delete a deck they've played, and the honest outcome is that the old session keeps its
/// row while the stats read — which inner-joins `decks` — simply stops counting it, rather
/// than the delete failing or the session vanishing.
///
/// `Eq` is derivable — every column is an integer, string, or timestamp.
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "life_session_players")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Parent `life_sessions.id`.
    pub session_id: i32,
    /// Seat order within the session, 0-based and gap-free after any edit.
    pub position: i32,
    pub name: String,
    /// One of the owner's `decks.id` this seat played, or null. No FK — see the type doc.
    pub deck_id: Option<i32>,
    /// The `cards.id` of the commander this seat played, or null — the alternative to
    /// `deck_id` for a player whose deck you'll never have. FK-less and orphan-tolerant for the
    /// same reason, and for one more: a catalog re-import can remove the card row.
    pub commander_card_id: Option<i32>,
    /// The total this seat started on (the session default unless overridden).
    pub starting_life: i32,
    /// The seat's current life total — the folded result of its `life_events`.
    pub life: i32,
    /// Screen rotation for the seat's tile, in degrees: `0`, `90`, `180` or `270`, so a
    /// player sitting across the table reads their own total right-side-up.
    pub rotation: i32,
    /// `"none"` while the game is active, then `"win"` / `"loss"` / `"draw"`.
    pub result: String,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// The two joins the per-deck record needs. `Deck` is join metadata only — the column
/// deliberately carries no database foreign key (see the type doc), and a join needs none.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::life_session::Entity",
        from = "Column::SessionId",
        to = "super::life_session::Column::Id"
    )]
    Session,
    #[sea_orm(
        belongs_to = "super::deck::Entity",
        from = "Column::DeckId",
        to = "super::deck::Column::Id"
    )]
    Deck,
}

impl Related<super::life_session::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Session.def()
    }
}

impl Related<super::deck::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Deck.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
