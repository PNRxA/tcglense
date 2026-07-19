//! Interactive terminal UI (ratatui): browse the catalog (games → sets → cards →
//! detail), search, and manage the collection / wish list / decks for a game.
//!
//! Navigation is a screen stack: Enter descends, Esc/Backspace goes back, `q`
//! quits. When signed in, `+`/`-` adjust owned counts, `f`/`F` foil counts, `w`
//! adds to the wish list.

mod ui;

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::DefaultTerminal;
use ratatui::widgets::ListState;

use crate::client::Client;
use crate::commands::Ctx;
use crate::models::*;

/// Which per-game holdings surface a Holdings screen is showing.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum HoldSurface {
    Collection,
    Wishlist,
}

impl HoldSurface {
    fn base(&self, game: &str) -> String {
        match self {
            HoldSurface::Collection => format!("/api/collection/{game}"),
            HoldSurface::Wishlist => format!("/api/wishlist/{game}"),
        }
    }
    fn label(&self) -> &'static str {
        match self {
            HoldSurface::Collection => "Collection",
            HoldSurface::Wishlist => "Wish list",
        }
    }
}

/// How a Cards screen was populated (so paging can refetch).
#[derive(Clone)]
pub(super) enum CardSource {
    Set { code: String, query: Option<String> },
    Search { query: String },
}

/// The per-game action menu.
pub(super) const GAME_MENU: [&str; 6] = [
    "Browse sets",
    "Search cards",
    "Collection",
    "Wish list",
    "Decks",
    "Account",
];

/// A navigable screen. Each list screen owns its selection state so back
/// navigation restores it.
pub(super) enum Screen {
    Games {
        items: Vec<Game>,
        state: ListState,
    },
    GameMenu {
        game: String,
        state: ListState,
    },
    Sets {
        game: String,
        items: Vec<CardSet>,
        state: ListState,
    },
    Cards {
        game: String,
        title: String,
        source: CardSource,
        items: Vec<Card>,
        page: i64,
        total: i64,
        has_more: bool,
        state: ListState,
    },
    CardDetail {
        card: Box<Card>,
    },
    Holdings {
        game: String,
        surface: HoldSurface,
        items: Vec<CollectionEntry>,
        page: i64,
        total: i64,
        has_more: bool,
        state: ListState,
    },
    Decks {
        game: String,
        items: Vec<Deck>,
        state: ListState,
    },
    DeckDetail {
        deck: Box<DeckDetail>,
    },
    Account {
        lines: Vec<String>,
    },
    Message {
        title: String,
        lines: Vec<String>,
    },
}

pub(super) struct App {
    client: Client,
    base_url: String,
    authed: bool,
    stack: Vec<Screen>,
    status: String,
    input: Option<InputState>,
    should_quit: bool,
}

/// Active text-entry overlay (card search).
pub(super) struct InputState {
    pub prompt: String,
    pub value: String,
    purpose: InputPurpose,
}

#[derive(Clone)]
enum InputPurpose {
    SearchCards { game: String },
}

fn new_state(len: usize) -> ListState {
    let mut s = ListState::default();
    if len > 0 {
        s.select(Some(0));
    }
    s
}

pub async fn run(ctx: Ctx) -> Result<()> {
    let mut terminal = ratatui::try_init().map_err(|e| {
        anyhow::anyhow!("the TUI needs an interactive terminal (TTY): {e}. Use the one-shot commands instead (see `tcglense --help`).")
    })?;
    let mut app = App::new(ctx).await;
    let result = app.event_loop(&mut terminal).await;
    let _ = ratatui::try_restore();
    result
}

impl App {
    async fn new(ctx: Ctx) -> App {
        let authed = ctx.client.current_auth().await.is_some();
        let base_url = ctx.client.base_url().to_string();
        let mut app = App {
            client: ctx.client,
            base_url,
            authed,
            stack: Vec::new(),
            status: String::new(),
            input: None,
            should_quit: false,
        };
        match app.fetch_games().await {
            Ok(items) => {
                let state = new_state(items.len());
                app.stack.push(Screen::Games { items, state });
                app.status = "↑/↓ move · Enter open · q quit · ? help".into();
            }
            Err(e) => app.stack.push(Screen::Message {
                title: "Could not load games".into(),
                lines: vec![e.to_string(), String::new(), "Press q to quit.".into()],
            }),
        }
        app
    }

    async fn event_loop(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|f| ui::render(f, self))?;
            if !event::poll(Duration::from_millis(200))? {
                continue;
            }
            if let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                self.on_key(key).await?;
            }
            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    async fn on_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.input.is_some() {
            self.on_input_key(key).await;
            return Ok(());
        }
        match key.code {
            KeyCode::Char('q') => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('?') => {
                self.push_help();
                return Ok(());
            }
            KeyCode::Esc | KeyCode::Backspace | KeyCode::Left => {
                self.pop();
                return Ok(());
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_selection(-1);
                return Ok(());
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(1);
                return Ok(());
            }
            KeyCode::PageUp => {
                self.move_selection(-10);
                return Ok(());
            }
            KeyCode::PageDown => {
                self.move_selection(10);
                return Ok(());
            }
            _ => {}
        }

        // Context-specific keys.
        match key.code {
            KeyCode::Enter | KeyCode::Right => self.on_enter().await,
            KeyCode::Char('n') => self.page(1).await,
            KeyCode::Char('p') => self.page(-1).await,
            KeyCode::Char('+') | KeyCode::Char('=') => self.adjust(1, false).await,
            KeyCode::Char('-') | KeyCode::Char('_') => self.adjust(-1, false).await,
            KeyCode::Char('f') => self.adjust(1, true).await,
            KeyCode::Char('F') => self.adjust(-1, true).await,
            KeyCode::Char('r') => self.remove_selected().await,
            KeyCode::Char('w') => self.add_to_wishlist().await,
            _ => Ok(()),
        }
    }

    async fn on_input_key(&mut self, key: KeyEvent) {
        let Some(input) = self.input.as_mut() else {
            return;
        };
        match key.code {
            KeyCode::Enter => {
                let purpose = input.purpose.clone();
                let value = input.value.clone();
                self.input = None;
                match purpose {
                    InputPurpose::SearchCards { game } => {
                        if value.trim().is_empty() {
                            self.status = "Search cancelled.".into();
                        } else if let Err(e) = self.search_cards(&game, value).await {
                            self.status = format!("error: {e}");
                        }
                    }
                }
            }
            KeyCode::Esc => {
                self.input = None;
                self.status = "Cancelled.".into();
            }
            KeyCode::Backspace => {
                input.value.pop();
            }
            KeyCode::Char(c) => input.value.push(c),
            _ => {}
        }
    }

    // -- navigation helpers -------------------------------------------------

    pub(super) fn top(&self) -> &Screen {
        self.stack.last().expect("non-empty stack")
    }

    fn pop(&mut self) {
        if self.stack.len() > 1 {
            self.stack.pop();
        } else {
            self.status = "Press q to quit.".into();
        }
    }

    fn selected(&self) -> usize {
        match self.top() {
            Screen::Games { state, .. }
            | Screen::GameMenu { state, .. }
            | Screen::Sets { state, .. }
            | Screen::Cards { state, .. }
            | Screen::Holdings { state, .. }
            | Screen::Decks { state, .. } => state.selected().unwrap_or(0),
            _ => 0,
        }
    }

    fn list_len(&self) -> usize {
        match self.top() {
            Screen::Games { items, .. } => items.len(),
            Screen::GameMenu { .. } => GAME_MENU.len(),
            Screen::Sets { items, .. } => items.len(),
            Screen::Cards { items, .. } => items.len(),
            Screen::Holdings { items, .. } => items.len(),
            Screen::Decks { items, .. } => items.len(),
            _ => 0,
        }
    }

    fn move_selection(&mut self, delta: i64) {
        let len = self.list_len();
        if len == 0 {
            return;
        }
        let cur = self.selected() as i64;
        let next = (cur + delta).clamp(0, len as i64 - 1) as usize;
        if let Some(state) = self.top_state_mut() {
            state.select(Some(next));
        }
    }

    fn top_state_mut(&mut self) -> Option<&mut ListState> {
        match self.stack.last_mut()? {
            Screen::Games { state, .. }
            | Screen::GameMenu { state, .. }
            | Screen::Sets { state, .. }
            | Screen::Cards { state, .. }
            | Screen::Holdings { state, .. }
            | Screen::Decks { state, .. } => Some(state),
            _ => None,
        }
    }

    fn push_help(&mut self) {
        self.stack.push(Screen::Message {
            title: "Help".into(),
            lines: vec![
                "↑/↓ or j/k   move".into(),
                "Enter / →    open / descend".into(),
                "Esc / ← / ⌫  go back".into(),
                "n / p        next / previous page".into(),
                "/            (on card lists) search — via the menu".into(),
                "+ / -        adjust owned/wanted regular count".into(),
                "f / F        adjust foil count (+ / -)".into(),
                "w            add card to wish list".into(),
                "r            remove highlighted holding".into(),
                "q            quit".into(),
            ],
        });
    }

    // -- fetches ------------------------------------------------------------

    async fn fetch_games(&self) -> Result<Vec<Game>> {
        let body: DataBody<Vec<Game>> = self.client.get_json("/api/games", &[]).await?;
        Ok(body.data)
    }

    async fn on_enter(&mut self) -> Result<()> {
        let idx = self.selected();
        match self.top() {
            Screen::Games { items, .. } => {
                if let Some(g) = items.get(idx) {
                    let game = g.id.clone();
                    self.stack.push(Screen::GameMenu {
                        game,
                        state: new_state(GAME_MENU.len()),
                    });
                }
            }
            Screen::GameMenu { game, .. } => {
                let game = game.clone();
                self.open_menu_item(&game, idx).await?;
            }
            Screen::Sets { game, items, .. } => {
                if let Some(s) = items.get(idx) {
                    let (game, code) = (game.clone(), s.code.clone());
                    self.load_set_cards(&game, &code).await?;
                }
            }
            Screen::Cards { items, .. } => {
                if let Some(c) = items.get(idx) {
                    let card = Box::new(c.clone());
                    self.stack.push(Screen::CardDetail { card });
                }
            }
            Screen::Holdings { items, .. } => {
                if let Some(e) = items.get(idx) {
                    let card = Box::new(e.card.clone());
                    self.stack.push(Screen::CardDetail { card });
                }
            }
            Screen::Decks { game, items, .. } => {
                if let Some(d) = items.get(idx) {
                    let (game, id) = (game.clone(), d.id);
                    self.load_deck_detail(&game, id).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    async fn open_menu_item(&mut self, game: &str, idx: usize) -> Result<()> {
        match idx {
            0 => self.load_sets(game).await,
            1 => {
                self.input = Some(InputState {
                    prompt: format!("Search {game} cards (Scryfall syntax): "),
                    value: String::new(),
                    purpose: InputPurpose::SearchCards {
                        game: game.to_string(),
                    },
                });
                Ok(())
            }
            2 => self.load_holdings(game, HoldSurface::Collection).await,
            3 => self.load_holdings(game, HoldSurface::Wishlist).await,
            4 => self.load_decks(game).await,
            5 => self.load_account().await,
            _ => Ok(()),
        }
    }

    async fn load_sets(&mut self, game: &str) -> Result<()> {
        let body: DataBody<Vec<CardSet>> = self
            .client
            .get_json(&format!("/api/games/{game}/sets"), &[])
            .await?;
        let state = new_state(body.data.len());
        self.stack.push(Screen::Sets {
            game: game.to_string(),
            items: body.data,
            state,
        });
        Ok(())
    }

    async fn load_set_cards(&mut self, game: &str, code: &str) -> Result<()> {
        let source = CardSource::Set {
            code: code.to_string(),
            query: None,
        };
        let title = format!("{} · {}", game, code.to_uppercase());
        self.load_cards(game, title, source, 1, true).await
    }

    async fn search_cards(&mut self, game: &str, query: String) -> Result<()> {
        let title = format!("{game} · search “{query}”");
        let source = CardSource::Search { query };
        self.load_cards(game, title, source, 1, true).await
    }

    async fn load_cards(
        &mut self,
        game: &str,
        title: String,
        source: CardSource,
        page: i64,
        push: bool,
    ) -> Result<()> {
        let mut q: Vec<(&str, String)> =
            vec![("page", page.to_string()), ("page_size", "60".into())];
        let path = match &source {
            CardSource::Set { code, query } => {
                if let Some(query) = query {
                    q.push(("q", query.clone()));
                }
                format!("/api/games/{game}/sets/{code}/cards")
            }
            CardSource::Search { query } => {
                q.push(("q", query.clone()));
                format!("/api/games/{game}/cards")
            }
        };
        let page_data: Page<Card> = self.client.get_json(&path, &q).await?;
        let state = new_state(page_data.data.len());
        let screen = Screen::Cards {
            game: game.to_string(),
            title,
            source,
            items: page_data.data,
            page: page_data.page,
            total: page_data.total,
            has_more: page_data.has_more,
            state,
        };
        if push {
            self.stack.push(screen);
        } else {
            *self.stack.last_mut().unwrap() = screen;
        }
        Ok(())
    }

    async fn load_holdings(&mut self, game: &str, surface: HoldSurface) -> Result<()> {
        self.fetch_holdings(game, surface, 1, true).await
    }

    async fn fetch_holdings(
        &mut self,
        game: &str,
        surface: HoldSurface,
        page: i64,
        push: bool,
    ) -> Result<()> {
        if !self.authed {
            self.status = format!("Sign in to view your {}.", surface.label().to_lowercase());
            return Ok(());
        }
        let q = vec![("page", page.to_string()), ("page_size", "60".into())];
        let page_data: Page<CollectionEntry> =
            self.client.get_json(&surface.base(game), &q).await?;
        let state = new_state(page_data.data.len());
        let screen = Screen::Holdings {
            game: game.to_string(),
            surface,
            items: page_data.data,
            page: page_data.page,
            total: page_data.total,
            has_more: page_data.has_more,
            state,
        };
        if push {
            self.stack.push(screen);
        } else {
            *self.stack.last_mut().unwrap() = screen;
        }
        Ok(())
    }

    async fn load_decks(&mut self, game: &str) -> Result<()> {
        if !self.authed {
            self.status = "Sign in to view your decks.".into();
            return Ok(());
        }
        let body: DataBody<Vec<Deck>> = self
            .client
            .get_json(&format!("/api/decks/{game}"), &[])
            .await?;
        let state = new_state(body.data.len());
        self.stack.push(Screen::Decks {
            game: game.to_string(),
            items: body.data,
            state,
        });
        Ok(())
    }

    async fn load_deck_detail(&mut self, game: &str, deck_id: i64) -> Result<()> {
        let deck: DeckDetail = self
            .client
            .get_json(&format!("/api/decks/{game}/{deck_id}"), &[])
            .await?;
        self.stack.push(Screen::DeckDetail {
            deck: Box::new(deck),
        });
        Ok(())
    }

    async fn load_account(&mut self) -> Result<()> {
        let mut lines = vec![format!("Base URL : {}", self.base_url)];
        if self.authed {
            match self
                .client
                .get_json::<serde_json::Value>("/api/auth/me", &[])
                .await
            {
                Ok(v) => {
                    if let Some(u) = v.get("user") {
                        lines.push(format!(
                            "Email    : {}",
                            u.get("email").and_then(|e| e.as_str()).unwrap_or("?")
                        ));
                        lines.push(format!(
                            "Handle   : {}",
                            u.get("handle").and_then(|e| e.as_str()).unwrap_or("(none)")
                        ));
                        lines.push(format!(
                            "Currency : {}",
                            u.get("currency").and_then(|e| e.as_str()).unwrap_or("?")
                        ));
                    }
                }
                Err(e) => lines.push(format!("(/me failed: {e})")),
            }
        } else {
            lines.push("Not signed in — run `tcglense login` first.".into());
        }
        self.stack.push(Screen::Account { lines });
        Ok(())
    }

    // -- paging & mutations -------------------------------------------------

    async fn page(&mut self, delta: i64) -> Result<()> {
        match self.top() {
            Screen::Cards {
                game,
                title,
                source,
                page,
                has_more,
                ..
            } => {
                let next = page + delta;
                if next < 1 || (delta > 0 && !has_more) {
                    self.status = "No more pages.".into();
                    return Ok(());
                }
                let (game, title, source) = (game.clone(), title.clone(), source.clone());
                self.load_cards(&game, title, source, next, false).await?;
            }
            Screen::Holdings {
                game,
                surface,
                page,
                has_more,
                ..
            } => {
                let next = page + delta;
                if next < 1 || (delta > 0 && !has_more) {
                    self.status = "No more pages.".into();
                    return Ok(());
                }
                let (game, surface) = (game.clone(), *surface);
                self.fetch_holdings(&game, surface, next, false).await?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Adjust the highlighted card's owned/wanted count by `delta` (regular or foil).
    async fn adjust(&mut self, delta: i64, foil: bool) -> Result<()> {
        if !self.authed {
            self.status = "Sign in to edit holdings.".into();
            return Ok(());
        }
        let idx = self.selected();
        match self.top() {
            Screen::Cards { game, items, .. } => {
                let Some(card) = items.get(idx) else {
                    return Ok(());
                };
                let (game, id, name) = (game.clone(), card.id.clone(), card.name.clone());
                let base = HoldSurface::Collection.base(&game);
                self.apply_delta(&base, &id, delta, foil).await?;
                self.status = format!("Updated {name} in collection.");
            }
            Screen::Holdings {
                game,
                surface,
                items,
                ..
            } => {
                let Some(entry) = items.get(idx) else {
                    return Ok(());
                };
                let base = surface.base(game);
                let id = entry.card.id.clone();
                let new_reg = if foil {
                    entry.quantity
                } else {
                    (entry.quantity + delta).max(0)
                };
                let new_foil = if foil {
                    (entry.foil_quantity + delta).max(0)
                } else {
                    entry.foil_quantity
                };
                let q = self.put_counts(&base, &id, new_reg, new_foil).await?;
                self.update_holding_row(idx, q);
            }
            _ => {}
        }
        Ok(())
    }

    async fn add_to_wishlist(&mut self) -> Result<()> {
        if !self.authed {
            self.status = "Sign in to edit the wish list.".into();
            return Ok(());
        }
        let idx = self.selected();
        if let Screen::Cards { game, items, .. } = self.top() {
            let Some(card) = items.get(idx) else {
                return Ok(());
            };
            let (game, id, name) = (game.clone(), card.id.clone(), card.name.clone());
            let base = HoldSurface::Wishlist.base(&game);
            self.apply_delta(&base, &id, 1, false).await?;
            self.status = format!("Added {name} to the wish list.");
        }
        Ok(())
    }

    async fn remove_selected(&mut self) -> Result<()> {
        if !self.authed {
            return Ok(());
        }
        let idx = self.selected();
        if let Screen::Holdings {
            game,
            surface,
            items,
            ..
        } = self.top()
        {
            let Some(entry) = items.get(idx) else {
                return Ok(());
            };
            let base = surface.base(game);
            let id = entry.card.id.clone();
            let q = self.put_counts(&base, &id, 0, 0).await?;
            self.update_holding_row(idx, q);
            self.status = "Removed.".into();
        }
        Ok(())
    }

    /// GET the current counts for a card then PUT the adjusted value.
    async fn apply_delta(&self, base: &str, id: &str, delta: i64, foil: bool) -> Result<()> {
        let path = format!("{base}/cards/{id}");
        let current: CollectionQuantities = self.client.get_json(&path, &[]).await?;
        let (reg, f) = if foil {
            (current.quantity, (current.foil_quantity + delta).max(0))
        } else {
            ((current.quantity + delta).max(0), current.foil_quantity)
        };
        let body = serde_json::json!({ "quantity": reg, "foil_quantity": f });
        let _: CollectionQuantities = self.client.put_json(&path, body).await?;
        Ok(())
    }

    async fn put_counts(
        &self,
        base: &str,
        id: &str,
        reg: i64,
        foil: i64,
    ) -> Result<CollectionQuantities> {
        let path = format!("{base}/cards/{id}");
        let body = serde_json::json!({ "quantity": reg, "foil_quantity": foil });
        self.client.put_json(&path, body).await
    }

    /// Reflect a mutation into the in-memory holdings row (removing zeroed rows).
    fn update_holding_row(&mut self, idx: usize, q: CollectionQuantities) {
        if let Some(Screen::Holdings { items, state, .. }) = self.stack.last_mut() {
            if idx >= items.len() {
                return;
            }
            if q.quantity == 0 && q.foil_quantity == 0 {
                items.remove(idx);
                let len = items.len();
                if len == 0 {
                    state.select(None);
                } else {
                    state.select(Some(idx.min(len - 1)));
                }
            } else {
                items[idx].quantity = q.quantity;
                items[idx].foil_quantity = q.foil_quantity;
            }
        }
    }
}
