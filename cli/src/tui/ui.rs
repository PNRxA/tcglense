//! Rendering for the interactive TUI: a header breadcrumb, a body that varies by
//! screen (lists or detail panes), a status/hints footer, and the search overlay.

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};

use super::{App, GAME_MENU, HoldSurface, InputState, Screen};
use crate::models::*;

const HL: Color = Color::Cyan;

pub(super) fn render(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .split(area);

    render_header(f, chunks[0], app);
    let footer = footer_text(app);
    render_footer(f, chunks[2], &footer);
    render_body(f, chunks[1], app);

    if let Some(input) = app.input.as_ref() {
        render_input(f, area, input);
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App) {
    let crumbs = breadcrumb(app);
    let auth = if app.authed {
        "● signed in"
    } else {
        "○ anonymous"
    };
    let cols = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(auth.len() as u16 + 1),
    ])
    .split(area);
    let title = Line::from(vec![
        Span::styled(" TCGLense ", Style::default().fg(Color::Black).bg(HL)),
        Span::raw(" "),
        Span::styled(crumbs, Style::default().add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(title), cols[0]);
    f.render_widget(
        Paragraph::new(auth)
            .alignment(Alignment::Right)
            .style(Style::default().fg(if app.authed {
                Color::Green
            } else {
                Color::DarkGray
            })),
        cols[1],
    );
}

fn render_footer(f: &mut Frame, area: Rect, text: &str) {
    f.render_widget(
        Paragraph::new(text).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_body(f: &mut Frame, area: Rect, app: &mut App) {
    match app.stack.last_mut().expect("non-empty stack") {
        Screen::Games { items, state } => {
            let rows: Vec<ListItem> = items
                .iter()
                .map(|g| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<6}", g.id), Style::default().fg(HL)),
                        Span::raw(format!("{}  ", g.name)),
                        Span::styled(
                            format!("({})", g.publisher),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect();
            list(f, area, "Games", rows, state);
        }
        Screen::GameMenu { game, state } => {
            let rows: Vec<ListItem> = GAME_MENU
                .iter()
                .map(|m| ListItem::new(Line::raw(*m)))
                .collect();
            list(
                f,
                area,
                &format!("Game: {}", game.to_uppercase()),
                rows,
                state,
            );
        }
        Screen::Sets { items, state, .. } => {
            let rows: Vec<ListItem> = items
                .iter()
                .map(|s| {
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("{:<8}", s.code.to_uppercase()),
                            Style::default().fg(HL),
                        ),
                        Span::raw(clip(&s.name, 40)),
                        Span::styled(
                            format!(
                                "  {} cards  {}",
                                s.card_count,
                                s.released_at.as_deref().unwrap_or("")
                            ),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect();
            list(f, area, "Sets", rows, state);
        }
        Screen::Cards {
            items,
            state,
            title,
            ..
        } => {
            let rows: Vec<ListItem> = items.iter().map(card_row).collect();
            let title = title.clone();
            list(f, area, &title, rows, state);
        }
        Screen::CardDetail { card } => {
            f.render_widget(
                Paragraph::new(card_detail_text(card))
                    .block(bordered(&card.name))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Screen::Holdings {
            items,
            state,
            surface,
            ..
        } => {
            let rows: Vec<ListItem> = items.iter().map(holding_row).collect();
            let label = surface.label().to_string();
            list(f, area, &label, rows, state);
        }
        Screen::Decks { items, state, .. } => {
            let rows: Vec<ListItem> = items
                .iter()
                .map(|d| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:<5}", d.id), Style::default().fg(Color::DarkGray)),
                        Span::raw(clip(&d.name, 36)),
                        Span::styled(
                            format!(
                                "  {}  {} cards{}",
                                d.format.as_deref().unwrap_or("—"),
                                d.card_count,
                                if d.is_public { "  [public]" } else { "" }
                            ),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                })
                .collect();
            list(f, area, "Decks", rows, state);
        }
        Screen::DeckDetail { deck } => {
            f.render_widget(
                Paragraph::new(deck_detail_text(deck))
                    .block(bordered(&deck.name))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        Screen::Account { lines } => {
            let text = Text::from(
                lines
                    .iter()
                    .map(|l| Line::raw(l.clone()))
                    .collect::<Vec<_>>(),
            );
            f.render_widget(Paragraph::new(text).block(bordered("Account")), area);
        }
        Screen::Message { title, lines } => {
            let text = Text::from(
                lines
                    .iter()
                    .map(|l| Line::raw(l.clone()))
                    .collect::<Vec<_>>(),
            );
            let title = title.clone();
            f.render_widget(Paragraph::new(text).block(bordered(&title)), area);
        }
    }
}

fn list(
    f: &mut Frame,
    area: Rect,
    title: &str,
    rows: Vec<ListItem>,
    state: &mut ratatui::widgets::ListState,
) {
    let empty = rows.is_empty();
    let widget = List::new(rows)
        .block(bordered(title))
        .highlight_style(
            Style::default()
                .bg(HL)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    f.render_stateful_widget(widget, area, state);
    if empty {
        let inner = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(4),
            height: 1,
        };
        f.render_widget(
            Paragraph::new("(nothing here)").style(Style::default().fg(Color::DarkGray)),
            inner,
        );
    }
}

fn bordered(title: &str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .title(format!(" {title} "))
}

fn card_row(c: &Card) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::raw(format!("{:<32}", clip(&c.name, 31))),
        Span::styled(
            format!("{:<6}", c.set_code.to_uppercase()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw(format!("#{:<6}", clip(&c.collector_number, 5))),
        Span::styled(
            format!("{:<9}", c.rarity.as_deref().unwrap_or("")),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(price(&c.prices.usd), Style::default().fg(Color::Green)),
    ]))
}

fn holding_row(e: &CollectionEntry) -> ListItem<'static> {
    ListItem::new(Line::from(vec![
        Span::styled(
            format!("{:>3}×{:<3}", e.quantity, foil_label(e.foil_quantity)),
            Style::default().fg(HL),
        ),
        Span::raw(format!(" {:<32}", clip(&e.card.name, 31))),
        Span::styled(
            format!("{:<6}", e.card.set_code.to_uppercase()),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(price(&e.card.prices.usd), Style::default().fg(Color::Green)),
    ]))
}

fn foil_label(foil: i64) -> String {
    if foil > 0 {
        format!("{foil}f")
    } else {
        "-".to_string()
    }
}

fn card_detail_text(c: &Card) -> Text<'static> {
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::styled(
        c.name.clone(),
        Style::default().add_modifier(Modifier::BOLD),
    ));
    lines.push(Line::raw(format!(
        "{} · {} #{} · {}",
        c.set_name,
        c.set_code.to_uppercase(),
        c.collector_number,
        c.rarity.as_deref().unwrap_or("—")
    )));
    if let Some(mc) = &c.mana_cost
        && !mc.is_empty()
    {
        lines.push(Line::raw(format!(
            "Mana {mc}  ·  CMC {}",
            c.cmc.unwrap_or(0.0)
        )));
    }
    if let Some(tl) = &c.type_line {
        lines.push(Line::raw(tl.clone()));
    }
    if let (Some(p), Some(t)) = (&c.power, &c.toughness) {
        lines.push(Line::raw(format!("{p}/{t}")));
    }
    lines.push(Line::raw(""));
    if let Some(ot) = &c.oracle_text {
        for l in ot.lines() {
            lines.push(Line::raw(l.to_string()));
        }
    }
    for face in &c.faces {
        lines.push(Line::raw(""));
        if let Some(n) = &face.name {
            lines.push(Line::styled(
                n.clone(),
                Style::default().add_modifier(Modifier::BOLD),
            ));
        }
        if let Some(tl) = &face.type_line {
            lines.push(Line::raw(tl.clone()));
        }
        if let Some(ot) = &face.oracle_text {
            for l in ot.lines() {
                lines.push(Line::raw(l.to_string()));
            }
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!(
            "USD {} · Foil {} · EUR {} · TIX {}",
            price(&c.prices.usd),
            price(&c.prices.usd_foil),
            c.prices.eur.as_deref().unwrap_or("—"),
            c.prices.tix.as_deref().unwrap_or("—"),
        ),
        Style::default().fg(Color::Green),
    ));
    lines.push(Line::styled(
        format!("id {}", c.id),
        Style::default().fg(Color::DarkGray),
    ));
    Text::from(lines)
}

fn deck_detail_text(d: &DeckDetail) -> Text<'static> {
    use std::collections::HashMap;
    let mut lines: Vec<Line> = Vec::new();
    lines.push(Line::raw(format!(
        "format {} · {} cards · value {} · public {}",
        d.format.as_deref().unwrap_or("—"),
        d.summary.total_cards,
        price(&d.summary.total_value_usd),
        d.is_public
    )));
    let mut by_section: HashMap<i64, Vec<&DeckCardEntry>> = HashMap::new();
    for c in &d.cards {
        by_section.entry(c.section_id).or_default().push(c);
    }
    for section in &d.sections {
        let Some(cards) = by_section.get(&section.id) else {
            continue;
        };
        if cards.is_empty() {
            continue;
        }
        lines.push(Line::raw(""));
        lines.push(Line::styled(
            format!("== {} ==", section.name),
            Style::default().fg(HL).add_modifier(Modifier::BOLD),
        ));
        for c in cards {
            let foil = if c.foil_quantity > 0 {
                format!(" (+{} foil)", c.foil_quantity)
            } else {
                String::new()
            };
            lines.push(Line::raw(format!(
                "  {}× {}{}",
                c.quantity, c.card.name, foil
            )));
        }
    }
    Text::from(lines)
}

fn render_input(f: &mut Frame, area: Rect, input: &InputState) {
    let popup = centered_rect(70, 3, area);
    f.render_widget(Clear, popup);
    let line = Line::from(vec![
        Span::styled(&input.prompt, Style::default().fg(Color::Yellow)),
        Span::raw(&input.value),
        Span::styled("▏", Style::default().fg(HL)),
    ]);
    f.render_widget(
        Paragraph::new(line).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" search — Enter to run · Esc to cancel "),
        ),
        popup,
    );
}

fn footer_text(app: &App) -> String {
    let (hints, paging) = match app.top() {
        Screen::Cards { page, total, .. } => (
            "Enter detail · n/p page · +/- own · f/F foil · w wish · Esc back",
            format!("p{page} · {total} cards  —  "),
        ),
        Screen::Holdings { page, total, .. } => (
            "Enter detail · +/- · f/F · r remove · n/p page · Esc back",
            format!("p{page} · {total} held  —  "),
        ),
        Screen::Decks { .. } => ("Enter open · Esc back", String::new()),
        Screen::CardDetail { .. } | Screen::DeckDetail { .. } | Screen::Account { .. } => {
            ("Esc back · q quit", String::new())
        }
        _ => (
            "↑/↓ move · Enter open · Esc back · q quit · ? help",
            String::new(),
        ),
    };
    if app.status.is_empty() {
        format!(" {paging}{hints}")
    } else {
        format!(" {}  —  {paging}{hints}", app.status)
    }
}

fn breadcrumb(app: &App) -> String {
    app.stack.iter().map(crumb).collect::<Vec<_>>().join(" › ")
}

fn crumb(s: &Screen) -> String {
    match s {
        Screen::Games { .. } => "Games".into(),
        Screen::GameMenu { game, .. } => game.to_uppercase(),
        Screen::Sets { .. } => "Sets".into(),
        Screen::Cards { title, .. } => clip(title, 30),
        Screen::CardDetail { card } => clip(&card.name, 24),
        Screen::Holdings { surface, .. } => match surface {
            HoldSurface::Collection => "Collection".into(),
            HoldSurface::Wishlist => "Wish list".into(),
        },
        Screen::Decks { .. } => "Decks".into(),
        Screen::DeckDetail { deck } => clip(&deck.name, 24),
        Screen::Account { .. } => "Account".into(),
        Screen::Message { title, .. } => title.clone(),
    }
}

fn centered_rect(percent_x: u16, height: u16, area: Rect) -> Rect {
    let w = area.width * percent_x / 100;
    let x = area.x + (area.width.saturating_sub(w)) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: w,
        height,
    }
}

fn clip(s: &str, max: usize) -> String {
    crate::output::truncate(s, max)
}

fn price(v: &Option<String>) -> String {
    match v {
        Some(p) => format!("${p}"),
        None => "—".into(),
    }
}
