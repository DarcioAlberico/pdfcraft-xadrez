//! Chess ▸ Make the book's bookmarks… (plan A9-N6, finding A8-13): the book's outline from its
//! reading, shown before it is made, then made in one step Edit ▸ Undo takes back. The tool
//! `chess_bookmarks` does the same without the interface.
//!
//! The whole book is read first, a long job like Export ([`PdfCraftApp::read_book_for_bookmarks`]).
//! The preview, a dialog, shows the tree the engine makes of it
//! ([`pdfcraft_engine::chess::pdfcraft_chessvision::doc::bookmarks::outline`]): the chapters (the
//! book's headings), the games whose header names both players (numbered and titled as an index
//! prints them) and the exercises (the diagrams a caption numbers), games and exercises under the
//! chapter before them. The kinds can be left out and the bookmark that holds them named (the
//! interface's "Chess" and "Exercise" are in its language). **Add the bookmarks** makes a new
//! first bookmark holding the tree (`Edit::AddBookmarkTree`, "Add chess bookmarks" in Undo); the
//! bookmarks there stay. A document that doesn't allow assembling refuses it with the engine's
//! reason, as every bookmark edit.

use egui::RichText;
use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::ChessDoc;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::bookmarks::{self, BookEntry, EntryKind, Include, MAX_TITLE_CHARS};

use crate::PdfCraftApp;

/// The bookmarks of a book read, shown before they are made.
pub(crate) struct BookmarksPreview {
    doc: DocId,
    /// The document's edit generation when it was read: the bookmarks go to the pages read.
    generation: u64,
    read: ChessDoc,
    include: Include,
    /// The name of the bookmark that holds them.
    title: String,
    /// The outline for `include`, made again when it changes.
    entries: Vec<BookEntry>,
}

impl BookmarksPreview {
    fn new(doc: DocId, generation: u64, read: ChessDoc) -> BookmarksPreview {
        let include = Include::default();
        let entries = bookmarks::outline(&read, include, tl!("Exercise"));
        BookmarksPreview { doc, generation, read, include, title: tl!("Chess").to_owned(), entries }
    }

    fn count(&self, kind: EntryKind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

impl PdfCraftApp {
    /// The whole book `doc` was read for document `id`'s bookmarks: show them before they are made.
    pub(crate) fn preview_book_bookmarks(&mut self, id: DocId, doc: ChessDoc) {
        let Some(generation) = self.session.get(id).map(pdfcraft_engine::Document::edit_generation) else { return };
        self.chess.bookmarks = Some(BookmarksPreview::new(id, generation, doc));
    }

    /// Make the bookmarks `preview` shows, in one step of the document's history.
    fn add_book_bookmarks(&mut self, preview: &BookmarksPreview) {
        // The document changed meanwhile: its pages are not the ones read.
        if self.session.get(preview.doc).map(pdfcraft_engine::Document::edit_generation) != Some(preview.generation) {
            return self.notify(tl!("The document changed since it was read: make its bookmarks again.").to_string());
        }
        let entries: Vec<pdfcraft_engine::OutlineEntry> = preview
            .entries
            .iter()
            .filter_map(|e| {
                Some(pdfcraft_engine::OutlineEntry { level: e.level, title: e.title.clone(), page: usize::try_from(e.page).ok()?, element: None })
            })
            .collect();
        let n = entries.len();
        let title = preview.title.trim().chars().take(MAX_TITLE_CHARS).collect::<String>();
        let title = if title.is_empty() { tl!("Chess").to_owned() } else { title };
        let edit = pdfcraft_engine::Edit::AddBookmarkTree { title: title.clone(), entries };
        // On the document it was read from, shown or not.
        let applied = if self.active_ids().map(|(_, id)| id) == Some(preview.doc) {
            self.apply_edit(edit)
        } else {
            match self.session.apply(preview.doc, edit) {
                Ok(()) => true,
                Err(e) => {
                    self.notify_error(e);
                    false
                }
            }
        };
        if applied {
            self.notify(crate::i18n::fmt(
                tl!("Bookmarks added under “{title}”: {n}. Undo takes them away."),
                &[("title", &title), ("n", &n.to_string())],
            ));
        }
    }
}

/// The preview of the book's bookmarks, a dialog: the kinds, the bookmark's name and the tree,
/// then Add the bookmarks or Cancel (Esc, a click outside).
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    let Some(mut preview) = app.chess.bookmarks.take() else { return };
    // The document was closed meanwhile.
    if app.session.get(preview.doc).is_none() {
        return;
    }
    let t = crate::theme::Tokens::get(ctx);
    let mut answer = None;
    let shown = crate::widgets::show_titled_modal(ctx, "chess-bookmarks", tl!("The book's bookmarks"), |ui| {
        ui.set_max_width(520.0);
        let counts = crate::i18n::fmt(
            tl!("Chapters: {chapters} · Games: {games} · Exercises: {exercises}"),
            &[
                ("chapters", &preview.count(EntryKind::Chapter).to_string()),
                ("games", &preview.count(EntryKind::Game).to_string()),
                ("exercises", &preview.count(EntryKind::Exercise).to_string()),
            ],
        );
        ui.label(counts);
        let before = preview.include;
        ui.horizontal_wrapped(|ui| {
            ui.add(crate::widgets::checkbox(&mut preview.include.chapters, tl!("Chapters")));
            ui.add(crate::widgets::checkbox(&mut preview.include.games, tl!("Games")));
            ui.add(crate::widgets::checkbox(&mut preview.include.exercises, tl!("Exercises")));
        });
        if preview.include != before {
            preview.entries = bookmarks::outline(&preview.read, preview.include, tl!("Exercise"));
        }
        ui.horizontal(|ui| {
            let label = ui.label(tl!("Title of the bookmark"));
            ui.add(crate::widgets::line(&mut preview.title).desired_width(240.0).char_limit(MAX_TITLE_CHARS)).labelled_by(label.id);
        });
        ui.add_space(6.0);
        if preview.entries.is_empty() {
            ui.label(RichText::new(tl!("No chapter, game or exercise was found in the book.")).color(t.text_muted));
        } else {
            egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, true]).show(ui, |ui| {
                for e in &preview.entries {
                    ui.horizontal(|ui| {
                        ui.add_space(16.0 * f32::from(e.level.saturating_sub(1)));
                        // The page as the book numbers it (plan A9-T2b).
                        let page = usize::try_from(e.page).map(|p| app.page_label_in(preview.doc, p)).unwrap_or_default();
                        ui.label(&e.title);
                        ui.label(RichText::new(page).color(t.text_muted));
                    });
                }
            });
        }
        ui.add_space(10.0);
        ui.horizontal_wrapped(|ui| {
            let add = ui.add_enabled_ui(!preview.entries.is_empty(), |ui| crate::widgets::pill_button(ui, tl!("Add the bookmarks"), false)).inner;
            if add.clicked() {
                answer = Some(true);
            }
            // The keyboard starts on the answer that changes nothing.
            let cancel = crate::widgets::pill_button(ui, tl!("Cancel"), true);
            crate::widgets::dialog_focus(&cancel);
            if cancel.clicked() {
                answer = Some(false);
            }
        });
    });
    if answer.is_none() && shown.should_close() {
        answer = Some(false);
    }
    match answer {
        Some(true) => app.add_book_bookmarks(&preview),
        Some(false) => {}
        None => app.chess.bookmarks = Some(preview),
    }
}
