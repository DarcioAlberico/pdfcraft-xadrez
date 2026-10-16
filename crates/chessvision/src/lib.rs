//! pdfcraft-chessvision — L4 chess vision: find diagrams on pages, read them, read chess text.
//!
//! This crate turns pixels and page text into chess values from `pdfcraft-chess`. It depends on
//! no other L4 crate: the engine (L6) adapts `edit`'s page images and paths into this crate's own
//! input types and applies the results as edits.
//!
//! Boxes travel in PDF user space (points), always with the DPI they were measured at. Every
//! detector and reader returns diagrams in one reading order ([`order`]), so "diagram 3" means
//! the same diagram on screen, in an export and in a tool result.
//!
//! No model weights ship with PdfCraft: [`models`] loads the user's own `.onnx` files, and
//! without them detection still works and reading reports the models as missing.

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable, clippy::indexing_slicing)]

pub mod board;
pub mod chess_text;
pub mod coords;
pub mod detect;
pub mod doc;
pub mod glyphs;
mod imageops;
pub mod merge;
pub mod models;
pub mod order;
pub mod pieces;
pub mod raster;
