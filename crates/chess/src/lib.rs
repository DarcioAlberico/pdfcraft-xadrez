//! pdfcraft-chess — L0 chess knowledge, with no PDF in it.
//!
//! Everything here works on chess values (placements, moves, probabilities, shapes), never on
//! documents or files, so it is standalone, builds for wasm32 and can be tested without a page.
//! `pdfcraft-chessvision` turns pages into these values and the engine turns them back into edits.
//!
//! Diagrams read from books can be illegal (two white kings after a misread square), so positions
//! are held in this crate's own [`placement`] type. `cozy-chess` (MIT) is used only once a
//! position is known to be legal.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod catalog;
pub mod draw;
pub mod fen;
pub mod fragment;
pub mod game;
pub mod lichess;
pub mod nag;
pub mod placement;
pub mod recognize;
pub mod repair;
pub mod rules;
pub mod san;
pub mod svg;
pub mod uci;
