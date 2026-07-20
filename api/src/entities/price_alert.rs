use sea_orm::entity::prelude::*;

/// SeaORM entity for the `price_alerts` table (issue #525).
///
/// One row per user-created price alert on a single **card** or a single **sealed
/// product** for a game. When the target's current catalog price crosses the
/// `threshold` in `direction` (`"below"` or `"above"`), the evaluator
/// ([`crate::alerts`]) notifies the owner over their configured channels
/// ([`super::alert_channel`]).
///
/// Like the collection/wish-list holdings, the target is stored by its **internal**
/// catalog id (`card_id` or `product_id`; the HTTP surface resolves the provider's
/// external id before writing), and the row is orphan-tolerant: a catalog re-import
/// that removes the target simply makes the evaluator skip it (no crash), never a
/// dangling FK. Exactly one of `card_id` / `product_id` is set, matching `target_kind`.
///
/// `triggered` implements edge-triggered hysteresis so a persistently-crossed alert
/// notifies **once**, not every tick: the evaluator fires (and sets `triggered = true`)
/// only on the rising edge (`met && !triggered`), and re-arms (`triggered = false`) when
/// the price crosses back. `last_triggered_at` / `last_price` capture the last firing for
/// the UI and the message body.
///
/// `Eq` is derivable — every column is an integer, string, bool, or timestamp (prices
/// are kept as the decimal strings the provider sends, never `f64`).
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "price_alerts")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    /// Owning user (`users.id`). Deleting the user cascades the alert away.
    pub user_id: i32,
    /// Game discriminator, e.g. `"mtg"`.
    pub game: String,
    /// `"card"` or `"product"` — which catalog id below is populated.
    pub target_kind: String,
    /// `cards.id` when `target_kind == "card"`, else null.
    pub card_id: Option<i32>,
    /// `products.id` when `target_kind == "product"`, else null.
    pub product_id: Option<i32>,
    /// Which price column to watch: `"nonfoil"` / `"foil"` / `"etched"` (etched is
    /// card-only). Selects `price_usd` / `price_usd_foil` / `price_usd_etched`.
    pub finish: String,
    /// `"below"` (notify when price ≤ threshold) or `"above"` (notify when price ≥ threshold).
    pub direction: String,
    /// The USD threshold, kept as a decimal string like the catalog prices it compares to.
    pub threshold: String,
    /// Whether the alert is armed. A paused alert is never evaluated or delivered.
    pub is_active: bool,
    /// Edge-trigger latch: whether the alert is currently in its notified state (see the
    /// struct docs). Fired once on the rising edge; re-armed when the price crosses back.
    pub triggered: bool,
    /// When the alert last fired, or null if it never has.
    pub last_triggered_at: Option<DateTimeUtc>,
    /// The target's price (decimal string) at the last firing, for the UI / message body.
    pub last_price: Option<String>,
    pub created_at: DateTimeUtc,
    pub updated_at: DateTimeUtc,
}

/// Optional links to the alert's target row, used by the evaluator to LEFT-JOIN the
/// card/product `updated_at` for change-narrowing (issue #525). Exactly one side is set per
/// alert; the join is nullable on the other. No `impl Related` — these are used only via
/// `Relation::Card.def()` / `Relation::Product.def()` in a `.join(...)`.
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::card::Entity",
        from = "Column::CardId",
        to = "super::card::Column::Id"
    )]
    Card,
    #[sea_orm(
        belongs_to = "super::product::Entity",
        from = "Column::ProductId",
        to = "super::product::Column::Id"
    )]
    Product,
}

impl ActiveModelBehavior for ActiveModel {}
