//! **Tools** — the play-aid surfaces that sit beside the catalog rather than inside it.
//!
//! A tool is not a catalog read and not a holdings surface: it's something you *use* while
//! playing or building, backed by its own per-user rows. The SPA groups them under
//! `/tools` the way the rules-keyword glossary is grouped under `/keywords`, and the API
//! mirrors that with a `/api/tools/{game}/...` namespace so a second tool adds a module
//! here instead of a new top-level route family.
//!
//! Today there is one: [`life`], the life counter — a tracked game of MTG with its seats,
//! their life totals, the full gain/loss history, and (through the optional per-seat deck
//! link) a win/loss record per deck.

pub mod life;
