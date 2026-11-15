//! Chess ▸ Export (plan A8 P9c; plan A9-X2, finding R4-04): the export dialog, and what the export
//! said, with Open folder.
//!
//! One dialog, opened by Export book… (in the Diagrams panel's header and on the Text tab), in
//! place of the menus of radio items that closed at each choice. It holds the format (each named
//! by what it makes: "EPUB — e-book"), the pages, the book's language (which the reading may have
//! got wrong, A2-08), how the diagrams' text alternatives name the pieces (letters or words, for
//! speech), and for the PGN of games which diagrams get a game of their own and, when the document
//! keeps a book's PGN, where the games come from. Under them, what will come out ("Pages: 748 ·
//! Diagrams: 999 (not checked: 12) · Games from the book's PGN: 120 · Language: English"), and the
//! diagrams not checked yet, with Review first. The format, the PGN's choices and the descriptions
//! are remembered (the `chess_export` setting); the pages and the language are the document's, for
//! the session.
//!
//! Once the file is written, a window says where, with what the export noted (a diagram not
//! read, a font only named) and Open folder.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::export::{ChessExportOptions, ChessFormat};
use pdfcraft_engine::chess::pdfcraft_chess::describe::Naming;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::LangTag;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::PgnPositions;

use crate::PdfCraftApp;
use crate::chess_text_ui::TextAction;
use crate::theme::Tokens;

/// Where the PGN export takes its games from when the document keeps a book's PGN (plan
/// "Carregar PGN do livro", R6b): that PGN, every line kept, with the book's text as comments
/// (the default), or the text read from the PDF, as without one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PgnSource {
    #[default]
    Book,
    Text,
}

/// What an export writes: the engine's formats, and the book's exercises as a PGN of problems or
/// as Anki's cards (plan A9-N10, N10b), which are a PGN export to the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Pgn,
    Exercises,
    Anki,
    Epub,
    Html,
    Docx,
    Rtf,
    Markdown,
}

impl From<ChessFormat> for ExportFormat {
    fn from(format: ChessFormat) -> ExportFormat {
        match format {
            ChessFormat::Pgn => ExportFormat::Pgn,
            ChessFormat::Epub => ExportFormat::Epub,
            ChessFormat::Html => ExportFormat::Html,
            ChessFormat::Docx => ExportFormat::Docx,
            ChessFormat::Rtf => ExportFormat::Rtf,
            ChessFormat::Markdown => ExportFormat::Markdown,
        }
    }
}

impl ExportFormat {
    /// The formats, in the dialog's order.
    pub const ALL: [ExportFormat; 8] = [
        ExportFormat::Pgn,
        ExportFormat::Exercises,
        ExportFormat::Anki,
        ExportFormat::Epub,
        ExportFormat::Html,
        ExportFormat::Docx,
        ExportFormat::Rtf,
        ExportFormat::Markdown,
    ];

    /// The engine's format: the exercises are a PGN export.
    pub fn engine(self) -> ChessFormat {
        match self {
            ExportFormat::Pgn | ExportFormat::Exercises | ExportFormat::Anki => ChessFormat::Pgn,
            ExportFormat::Epub => ChessFormat::Epub,
            ExportFormat::Html => ChessFormat::Html,
            ExportFormat::Docx => ChessFormat::Docx,
            ExportFormat::Rtf => ChessFormat::Rtf,
            ExportFormat::Markdown => ChessFormat::Markdown,
        }
    }

    /// The file's extension: Anki imports a text file.
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Anki => "txt",
            other => other.engine().extension(),
        }
    }

    /// The format's name, as the save dialog's filter shows it.
    pub fn name(self) -> &'static str {
        match self {
            ExportFormat::Anki => "Anki",
            other => other.engine().name(),
        }
    }

    /// The format as the dialog names it: what it makes.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Pgn => tl!("PGN — games"),
            ExportFormat::Exercises => tl!("PGN — exercises"),
            ExportFormat::Anki => tl!("Anki — cards"),
            ExportFormat::Epub => tl!("EPUB — e-book"),
            ExportFormat::Html => tl!("HTML — web page"),
            // A program's and formats' own names, the same in every language.
            ExportFormat::Docx => "Word (.docx)",
            ExportFormat::Rtf => "RTF",
            ExportFormat::Markdown => "Markdown",
        }
    }

    /// What its item's tooltip adds, when its name does not say it all.
    fn tip(self) -> Option<&'static str> {
        match self {
            ExportFormat::Exercises => Some(tl!("The book's numbered diagrams with their solutions, as a standard PGN of problems.")),
            ExportFormat::Anki => Some(tl!("A text file Anki imports, a card per exercise, with the diagrams' pictures in a folder beside it.")),
            _ => None,
        }
    }

    /// Whether it says its diagrams in words (a text alternative), which the descriptions choice
    /// is for.
    fn describes(self) -> bool {
        matches!(self, ExportFormat::Epub | ExportFormat::Html | ExportFormat::Docx | ExportFormat::Rtf | ExportFormat::Markdown)
    }

    /// The name the setting keeps.
    fn token(self) -> &'static str {
        match self {
            ExportFormat::Pgn => "pgn",
            ExportFormat::Exercises => "exercises",
            ExportFormat::Anki => "anki",
            ExportFormat::Epub => "epub",
            ExportFormat::Html => "html",
            ExportFormat::Docx => "docx",
            ExportFormat::Rtf => "rtf",
            ExportFormat::Markdown => "markdown",
        }
    }

    fn from_token(token: &str) -> Option<ExportFormat> {
        ExportFormat::ALL.into_iter().find(|f| f.token() == token)
    }
}

/// The export dialog's choices that are remembered (plan A9-X2; the PGN's, P9c's open item): the
/// format, which diagrams the PGN gives a game of their own, where the PGN's games come from and
/// how the diagrams' descriptions name the pieces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportChoices {
    pub format: ExportFormat,
    pub positions: PgnPositions,
    pub source: PgnSource,
    pub naming: Naming,
}

impl ExportChoices {
    /// As the settings keep them.
    pub fn to_json(self) -> serde_json::Value {
        let positions = match self.positions {
            PgnPositions::All => "all",
            PgnPositions::Unused => "unused",
            PgnPositions::None => "none",
        };
        let source = if self.source == PgnSource::Text { "text" } else { "book" };
        serde_json::json!({ "format": self.format.token(), "positions": positions, "source": source, "descriptions": self.naming.name() })
    }

    /// Read back, each choice on its own: one this build does not know keeps its default.
    pub fn from_json(value: &serde_json::Value) -> ExportChoices {
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str).unwrap_or_default();
        ExportChoices {
            format: ExportFormat::from_token(text("format")).unwrap_or_default(),
            positions: PgnPositions::from_name(text("positions")).unwrap_or_default(),
            source: if text("source") == "text" { PgnSource::Text } else { PgnSource::Book },
            naming: Naming::from_name(text("descriptions")).unwrap_or_default(),
        }
    }
}

/// The pages an export reads, 1-based as the dialog shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pages {
    pub all: bool,
    pub from: usize,
    pub to: usize,
}

impl Default for Pages {
    fn default() -> Pages {
        Pages { all: true, from: 1, to: usize::MAX }
    }
}

impl Pages {
    /// The first and the last page (1-based) of the range in a document of `count` pages: never
    /// empty, never past its end (a document of no pages has the one page 1).
    fn bounds(self, count: usize) -> (usize, usize) {
        let last = count.max(1);
        let from = self.from.clamp(1, last);
        (from, self.to.clamp(from, last))
    }

    /// The pages (0-based) of a document of `count` pages; empty for all of them, as the engine
    /// takes it.
    pub fn pages(self, count: usize) -> Vec<usize> {
        let (from, to) = self.bounds(count);
        if self.all || count == 0 || (from == 1 && to == count) {
            return Vec::new();
        }
        (from..=to).map(|p| p.saturating_sub(1)).collect()
    }

    /// How many pages that is.
    fn count(self, count: usize) -> usize {
        let (from, to) = self.bounds(count);
        if self.all { count } else { (to - from).saturating_add(1).min(count) }
    }

    /// Whether 0-based page `page` of a document of `count` pages is among them.
    fn holds(self, page: usize, count: usize) -> bool {
        let (from, to) = self.bounds(count);
        self.all || (from..=to).contains(&page.saturating_add(1))
    }
}

/// What the export dialog holds of one document for the session: its pages and the language the
/// user said its book is in (`None`: as the reading decides).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DocExport {
    pub pages: Pages,
    pub lang: Option<Lang>,
}

/// What the book says its language is, when the dialog opened: what the reading will decide
/// unless the user says otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
enum BookSays {
    /// The PDF's `/Lang`, or the page the Text tab read, names it.
    Lang(Lang),
    /// The PDF's `/Lang` names a language with no notation of its own (`ja`): the export is
    /// written in it, as the tag says.
    Tag(LangTag),
    /// The reading will decide it from the book's text.
    FromText,
}

impl BookSays {
    /// The language as the dialog names it: by its own name, else by its tag.
    fn name(&self) -> Option<String> {
        match self {
            BookSays::Lang(lang) => Some(crate::chess_capture::endonym(*lang).to_owned()),
            BookSays::Tag(tag) => Some(tag.as_str().to_owned()),
            BookSays::FromText => None,
        }
    }
}

/// The export dialog while it is shown (plan A9-X2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportDialog {
    pub doc: DocId,
    pub choices: ExportChoices,
    pub pages: Pages,
    pub lang: Option<Lang>,
    /// The document's page count when it opened.
    page_count: usize,
    says: BookSays,
}

/// An answer to the dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogAnswer {
    /// Ask where, then write the file.
    Export,
    /// Open the first diagram to review in the pages chosen; nothing is exported.
    ReviewFirst,
    /// Cancel, Esc, or a click outside: nothing happens.
    Cancel,
}

/// What a written export said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Done {
    /// Where it was written; none on the web, where it was downloaded.
    pub path: Option<std::path::PathBuf>,
    /// "Exported to …".
    pub said: String,
    /// What the export noted, as the engine wrote it.
    pub notes: Vec<String>,
}

/// The language document `doc`'s PDF says it is in (its `/Lang`), as a tag; empty when it says
/// none, or something that is not a tag.
fn declared_lang(doc: &pdfcraft_engine::Document) -> LangTag {
    LangTag::parse(doc.initial_view().language.as_deref().unwrap_or_default())
}

/// Export book…: the one entry that opens the dialog, an item of the Text tab's ⋯ menu (plan
/// A9-I9).
pub fn entry(ui: &mut egui::Ui) -> Option<TextAction> {
    crate::widgets::menu_entry(ui, true, egui::Button::new(tl!("Export book…"))).clicked().then_some(TextAction::OpenExport)
}

impl PdfCraftApp {
    /// Export book…: open the export dialog for the active document.
    pub fn open_chess_export(&mut self) {
        let Some((_, id)) = self.active_ids() else { return };
        let Some(doc) = self.session.get(id) else { return };
        let page_count = doc.info.pages.len();
        // What the reading will take the book's language to be: its PDF's `/Lang`, whatever
        // language it names (the export writes it), else what the page the Text tab read says
        // (the prose of every page decides it in the export).
        let declared = declared_lang(doc);
        let kept = self.chess.doc(id).map(|d| d.export).unwrap_or_default();
        let read = self.chess.doc(id).and_then(|d| d.text.as_ref()).and_then(|t| t.chess.pages.first()).and_then(|p| p.text_lang.lang());
        let says = match (declared.lang(), declared.is_empty(), read) {
            (Some(lang), _, _) => BookSays::Lang(lang),
            (None, false, _) => BookSays::Tag(declared),
            (None, true, Some(lang)) => BookSays::Lang(lang),
            (None, true, None) => BookSays::FromText,
        };
        self.chess.export_dialog = Some(ExportDialog { doc: id, choices: self.chess.export, pages: kept.pages, lang: kept.lang, page_count, says });
    }

    /// Answer the export dialog. Its choices are kept whatever the answer but Cancel.
    pub fn answer_chess_export(&mut self, answer: DialogAnswer) {
        let Some(d) = self.chess.export_dialog.take() else { return };
        if answer == DialogAnswer::Cancel {
            return;
        }
        self.chess.export = d.choices;
        self.chess.doc_mut(d.doc).export = DocExport { pages: d.pages, lang: d.lang };
        // The document must still be the one the dialog was for.
        if !self.active_ids().is_some_and(|(_, id)| id == d.doc) {
            return;
        }
        match answer {
            DialogAnswer::Export => self.export_chess_dialog(d.choices.format),
            DialogAnswer::ReviewFirst => {
                let first = self.chess.results_of(d.doc).and_then(|r| r.review_queue().into_iter().find(|at| d.pages.holds(at.page, d.page_count)));
                self.chess.doc_mut(d.doc).list.filter = None;
                if let Some(at) = first {
                    self.edit_chess_diagram(at.page, at.index, true);
                }
            }
            DialogAnswer::Cancel => {}
        }
    }

    /// Set on `options` what the export dialog chose for exporting document `id` as `format`:
    /// the pages, the book's language, the descriptions, the PGN's choices and the book's PGN.
    pub(crate) fn chess_export_options(&mut self, id: DocId, format: ExportFormat, options: &mut ChessExportOptions) {
        let count = self.session.get(id).map_or(0, |d| d.info.pages.len());
        let kept = self.chess.doc(id).map(|d| d.export).unwrap_or_default();
        options.pages = kept.pages.pages(count);
        // The language chosen, with the region the PDF's `/Lang` gives it when it is that one
        // (`pt-BR` stays `pt-BR` when Português is chosen).
        let declared = self.session.get(id).map(declared_lang).unwrap_or_default();
        options.text_lang = kept.lang.map(|lang| if declared.lang() == Some(lang) { declared.clone() } else { LangTag::of(lang) });
        options.naming = self.chess.export.naming;
        options.positions = self.chess.export.positions;
        options.exercises = matches!(format, ExportFormat::Exercises | ExportFormat::Anki);
        options.anki = format == ExportFormat::Anki;
        options.reference = self.export_reference(id, format);
    }

    /// The export of `made` was written (to `path`; none on the web): say where, with its notes.
    pub(crate) fn chess_export_done(&mut self, made: &pdfcraft_engine::chess::export::ChessExport, path: Option<std::path::PathBuf>, said: String) {
        // An earlier export's error is stale now (plan A9-I17).
        self.settle_error("export");
        let mut notes = Vec::new();
        let (diagrams, read) = made.diagram_counts();
        let unread = diagrams.saturating_sub(read);
        if unread > 0 {
            notes.push(crate::i18n::fmt(tl!("Diagrams not read: {n}"), &[("n", &unread.to_string())]));
        }
        notes.extend(made.notes.iter().cloned());
        self.chess.export_done = Some(Done { path, said, notes });
    }

    /// Open folder: the system's file manager shows the file written.
    pub fn reveal_chess_export(&mut self) {
        let Some(path) = self.chess.export_done.as_ref().and_then(|d| d.path.clone()) else { return };
        self.chess.export_done = None;
        self.chess.last_revealed = Some(path.clone());
        #[cfg(not(target_arch = "wasm32"))]
        if self.ctx.is_some()
            && !self.run_inline
            && let Err(e) = crate::tab_context::reveal(&path)
        {
            self.post_error(e.to_string());
        }
    }

    /// What the dialog says will come out of document `d.doc`: the pages, the diagrams detected in
    /// them and those not checked yet, where the games come from, and the language. With the
    /// diagrams still to review in the pages chosen.
    fn chess_export_summary(&self, d: &ExportDialog) -> (String, usize) {
        let pages = d.pages.count(d.page_count);
        let mut parts = vec![crate::i18n::fmt(tl!("Pages: {n}"), &[("n", &pages.to_string())])];
        let mut to_review = 0;
        let mut counted = false;
        if let Some(r) = self.chess.results_of(d.doc) {
            to_review = r.review_queue().iter().filter(|at| d.pages.holds(at.page, d.page_count)).count();
            // Counted only when every page chosen was detected (the scan holds each page once):
            // else the export finds more.
            let chosen: Vec<_> = r.pages.iter().filter(|p| d.pages.holds(p.page, d.page_count)).collect();
            if chosen.len() == pages {
                let diagrams: usize = chosen.iter().map(|p| p.diagrams.len()).sum();
                let mut said = crate::i18n::fmt(tl!("Diagrams: {n}"), &[("n", &diagrams.to_string())]);
                if to_review > 0 {
                    said = format!("{said} ({})", crate::i18n::fmt(tl!("not checked: {n}"), &[("n", &to_review.to_string())]));
                }
                parts.push(said);
                counted = true;
            }
        }
        if !counted {
            parts.push(tl!("Diagrams: not detected yet").to_owned());
        }
        let format = d.choices.format;
        if !matches!(format, ExportFormat::Exercises | ExportFormat::Anki) {
            // The PGN of games is the book's PGN's games; every other format, the text read,
            // linked to it (plan A9-N2).
            let book = self.session.chess_book_pgn(d.doc).map(|p| p.index.games().len());
            match book {
                Some(n) if format == ExportFormat::Pgn && d.choices.source == PgnSource::Book => {
                    parts.push(crate::i18n::fmt(tl!("Games from the book's PGN: {n}"), &[("n", &n.to_string())]));
                }
                Some(_) if format != ExportFormat::Pgn => parts.push(tl!("Games: read from the text, linked to the book's PGN").to_owned()),
                _ => parts.push(tl!("Games: read from the text").to_owned()),
            }
            // The user's own games are linked first (plan A9-N2): they are in every format.
            let yours = self.chess.games.games_of(d.doc).iter().filter(|b| b.illegal.is_none() && !b.game.is_empty()).count();
            if yours > 0 {
                parts.push(crate::i18n::fmt(tl!("Your games, linked first: {n}"), &[("n", &yours.to_string())]));
            }
        }
        let lang = match d.lang {
            Some(lang) => crate::chess_capture::endonym(lang).to_owned(),
            None => d.says.name().unwrap_or_else(|| tl!("from the book's text").to_owned()),
        };
        parts.push(crate::i18n::fmt(tl!("Language: {lang}"), &[("lang", &lang)]));
        (parts.join(" · "), to_review)
    }
}

/// The width of the dialog's labels, in points: the fields line up after it.
const LABEL_WIDTH: f32 = 150.0;

/// One row of the dialog: `label` in its column, then what `field` adds, given the label's
/// response (to name the field by, `labelled_by`).
fn row<R>(ui: &mut egui::Ui, label: &str, field: impl FnOnce(&mut egui::Ui, &egui::Response) -> R) -> R {
    ui.horizontal_top(|ui| {
        let height = ui.spacing().interact_size.y;
        let named = ui.allocate_ui_with_layout(egui::vec2(LABEL_WIDTH, height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_size(egui::vec2(LABEL_WIDTH, height));
            ui.label(label)
        });
        field(ui, &named.inner)
    })
    .inner
}

/// The dialog's fields: the format, the pages, the language, the descriptions and the PGN's
/// choices, each named by the label beside it (plan A9-X2: role `Dialog`, fields `labelled_by`).
fn fields(ui: &mut egui::Ui, d: &mut ExportDialog, book_pgn: bool) {
    ui.spacing_mut().item_spacing.y = 8.0;
    row(ui, tl!("Format"), |ui, _| {
        ui.scope(|ui| {
            crate::widgets::radio_group(ui, tl!("Format"));
            // Two columns, read down each: the PGNs and Anki, then the books.
            let (left, right) = ExportFormat::ALL.split_at(4);
            ui.horizontal_top(|ui| {
                for column in [left, right] {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 4.0;
                        for &f in column {
                            let r = crate::widgets::radio_value(ui, &mut d.choices.format, f, f.label());
                            if f == d.choices.format {
                                crate::widgets::dialog_focus(&r);
                            }
                            if let Some(tip) = f.tip() {
                                r.on_hover_text(tip);
                            }
                        }
                    });
                    ui.add_space(12.0);
                }
            });
        });
    });
    row(ui, tl!("Pages"), |ui, _| {
        ui.scope(|ui| {
            crate::widgets::radio_group(ui, tl!("Pages"));
            ui.horizontal(|ui| {
                crate::widgets::radio_value(ui, &mut d.pages.all, true, tl!("All pages"));
                let from = crate::widgets::radio_value(ui, &mut d.pages.all, false, tl!("Pages from"));
                let last = d.page_count.max(1);
                d.pages.from = d.pages.from.clamp(1, last);
                d.pages.to = d.pages.to.clamp(d.pages.from, last);
                ui.add_enabled(!d.pages.all, egui::DragValue::new(&mut d.pages.from).range(1..=last)).labelled_by(from.id);
                let to = ui.label(tl!("to"));
                ui.add_enabled(!d.pages.all, egui::DragValue::new(&mut d.pages.to).range(1..=last)).labelled_by(to.id);
                d.pages.to = d.pages.to.max(d.pages.from);
            });
        });
    });
    row(ui, tl!("Book's language"), |ui, label| {
        let as_book = match d.says.name() {
            Some(lang) => crate::i18n::fmt(tl!("As the book says ({lang})"), &[("lang", &lang)]),
            None => tl!("As the book's text says").to_owned(),
        };
        let shown = d.lang.map_or_else(|| as_book.clone(), |l| crate::chess_capture::endonym(l).to_owned());
        let combo = egui::ComboBox::from_id_salt("chess-export-lang").selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut d.lang, None, as_book);
            for lang in Lang::ALL {
                ui.selectable_value(&mut d.lang, Some(lang), crate::chess_capture::endonym(lang));
            }
        });
        combo
            .response
            .labelled_by(label.id)
            .on_hover_text(tl!("The language the export is written in and says its diagrams in. Correct it when the book was read as another."));
    });
    let format = d.choices.format;
    if format.describes() {
        row(ui, tl!("Diagram descriptions"), |ui, _| {
            ui.scope(|ui| {
                crate::widgets::radio_group(ui, tl!("Diagram descriptions"));
                ui.horizontal(|ui| {
                    let tip = tl!("How a screen reader says each diagram's pieces.");
                    crate::widgets::radio_value(ui, &mut d.choices.naming, Naming::Letters, tl!("Letters (Kg1)")).on_hover_text(tip);
                    crate::widgets::radio_value(ui, &mut d.choices.naming, Naming::Words, tl!("Words (king g1)")).on_hover_text(tip);
                });
            });
        });
    }
    if format == ExportFormat::Pgn {
        row(ui, tl!("Diagrams as PGN games"), |ui, label| {
            let choices =
                [(PgnPositions::Unused, tl!("Only those not in a game")), (PgnPositions::All, tl!("All of them")), (PgnPositions::None, tl!("None"))];
            let shown = choices.iter().find(|(p, _)| *p == d.choices.positions).map_or("", |(_, n)| *n);
            let combo = egui::ComboBox::from_id_salt("chess-export-positions").selected_text(shown).show_ui(ui, |ui| {
                for (p, name) in choices {
                    ui.selectable_value(&mut d.choices.positions, p, name);
                }
            });
            combo.response.labelled_by(label.id).on_hover_text(tl!(
                "A diagram becomes a game that starts from its position. A diagram a game starts from or prints again is already in that game."
            ));
        });
        if book_pgn {
            row(ui, tl!("PGN games from"), |ui, label| {
                let choices = [(PgnSource::Book, tl!("The book's PGN, with the book's text")), (PgnSource::Text, tl!("The text read from the PDF"))];
                let shown = choices.iter().find(|(s, _)| *s == d.choices.source).map_or("", |(_, n)| *n);
                let combo = egui::ComboBox::from_id_salt("chess-export-source").selected_text(shown).show_ui(ui, |ui| {
                    for (s, name) in choices {
                        ui.selectable_value(&mut d.choices.source, s, name);
                    }
                });
                combo.response.labelled_by(label.id).on_hover_text(tl!(
                    "With the book's PGN, the PGN export writes its games, every line kept, with the book's text as comments and $220 where a diagram shows a move."
                ));
            });
        }
    }
}

/// The diagrams not checked yet in the pages chosen, inside the dialog, with Review first.
fn pending(ui: &mut egui::Ui, t: &Tokens, to_review: usize) -> bool {
    let mut review = false;
    egui::Frame::new().fill(t.warning_soft).corner_radius(egui::CornerRadius::same(6)).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let title = crate::i18n::fmt(tl!("Diagrams not checked yet: {n}"), &[("n", &to_review.to_string())]);
        ui.label(egui::RichText::new(title).strong().color(t.warning_text));
        ui.label(
            egui::RichText::new(tl!("They are exported as they were read, and a reading may be wrong. Review first opens the first of them."))
                .color(t.text),
        );
        ui.add_space(4.0);
        review = crate::widgets::pill_button(ui, tl!("Review first"), false).clicked();
    });
    review
}

/// The export dialog, or what the export said, while either is shown.
pub(crate) fn prompt(app: &mut PdfCraftApp, ctx: &egui::Context) {
    if let Some(mut d) = app.chess.export_dialog.take() {
        let book_pgn = app.session.chess_book_pgn(d.doc).is_some();
        let t = Tokens::get(ctx);
        let mut answer = None;
        let shown = crate::widgets::show_titled_modal(ctx, "chess-export", tl!("Export chess book"), |ui| {
            ui.set_max_width(560.0);
            fields(ui, &mut d, book_pgn);
            ui.add_space(8.0);
            ui.separator();
            let (summary, to_review) = app.chess_export_summary(&d);
            ui.label(summary);
            if to_review > 0 {
                ui.add_space(6.0);
                if pending(ui, &t, to_review) {
                    answer = Some(DialogAnswer::ReviewFirst);
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if crate::widgets::pill_button(ui, tl!("Export…"), true).clicked() {
                    answer = Some(DialogAnswer::Export);
                }
                if crate::widgets::pill_button(ui, tl!("Cancel"), false).clicked() {
                    answer = Some(DialogAnswer::Cancel);
                }
            });
        });
        if answer.is_none() && shown.should_close() {
            answer = Some(DialogAnswer::Cancel);
        }
        app.chess.export_dialog = Some(d);
        if let Some(a) = answer {
            app.answer_chess_export(a);
        }
        return;
    }
    let Some(d) = app.chess.export_done.as_ref() else { return };
    let (mut reveal, mut close) = (false, false);
    let shown = crate::widgets::show_titled_modal(ctx, "chess-export-done", &d.said, |ui| {
        ui.set_max_width(460.0);
        if !d.notes.is_empty() {
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(180.0).show(ui, |ui| {
                for note in &d.notes {
                    ui.label(note);
                }
            });
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if d.path.is_some() {
                let open = crate::widgets::pill_button(ui, tl!("Open folder"), true);
                crate::widgets::dialog_focus(&open);
                reveal = open.clicked();
            }
            let done = crate::widgets::pill_button(ui, tl!("Close"), d.path.is_none());
            crate::widgets::dialog_focus(&done);
            if done.clicked() {
                close = true;
            }
        });
    });
    if reveal {
        app.reveal_chess_export();
    } else if close || shown.should_close() {
        app.chess.export_done = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_are_all_or_a_range_within_the_document() {
        assert_eq!(Pages::default().pages(9), Vec::<usize>::new());
        assert_eq!(Pages::default().count(9), 9);
        let some = Pages { all: false, from: 2, to: 4 };
        assert_eq!(some.pages(9), [1, 2, 3]);
        assert_eq!(some.count(9), 3);
        assert!(some.holds(1, 9) && some.holds(3, 9) && !some.holds(0, 9) && !some.holds(4, 9));
        // The whole document as a range is every page; a range past the end stops at it.
        assert_eq!(Pages { all: false, from: 1, to: 9 }.pages(9), Vec::<usize>::new());
        assert_eq!(Pages { all: false, from: 8, to: usize::MAX }.pages(9), [7, 8]);
        // A range is never empty, which the engine would take for every page.
        assert_eq!(Pages { all: false, from: 0, to: 0 }.pages(3), [0]);
        assert_eq!(Pages { all: false, from: 5, to: 2 }.pages(9), [4]);
        assert_eq!(Pages { all: false, from: 12, to: 20 }.pages(9), [8]);
        assert_eq!(Pages { all: false, from: 5, to: 2 }.count(9), 1);
        assert_eq!(Pages::default().pages(0), Vec::<usize>::new());
        assert_eq!(Pages { all: false, from: 3, to: 4 }.pages(0), Vec::<usize>::new());
    }

    #[test]
    fn the_remembered_choices_read_back_and_unknown_ones_keep_their_defaults() {
        let c = ExportChoices { format: ExportFormat::Anki, positions: PgnPositions::None, source: PgnSource::Text, naming: Naming::Words };
        assert_eq!(ExportChoices::from_json(&c.to_json()), c);
        assert_eq!(ExportChoices::from_json(&serde_json::json!({ "format": "tiff", "positions": 3 })), ExportChoices::default());
        assert_eq!(ExportChoices::from_json(&serde_json::Value::Null), ExportChoices::default());
        for f in ExportFormat::ALL {
            assert_eq!(ExportFormat::from_token(f.token()), Some(f));
        }
        assert_eq!(ExportFormat::Anki.extension(), "txt");
        assert_eq!(ExportFormat::Exercises.engine(), ChessFormat::Pgn);
    }
}
