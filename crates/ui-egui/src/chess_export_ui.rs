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
//! diagrams not checked yet, with Review first. The format, the PGN's choices, the sheet's and the
//! descriptions are remembered (the `chess_export` setting); the pages and the language are the
//! document's, for the session.
//!
//! **The exercise sheet** (plan A9-N12b, A8-14): a new PDF of the book's numbered diagrams, 4, 6 or
//! 9 a page, with the solutions at the end or not ([`Session::chess_worksheet`]). Its summary
//! counts the exercises of the pages chosen **already read** — in this session or on disk —, off
//! the interface's thread and scanning nothing ([`Session::chess_worksheet_count`]: choosing the
//! format never reads an unread book), says how many are left out and why, and that the pages not
//! read yet are counted by the export. When every page chosen is read and none holds an exercise,
//! it says why and Export… is off.
//!
//! [`Session::chess_worksheet`]: pdfcraft_engine::Session::chess_worksheet
//! [`Session::chess_worksheet_count`]: pdfcraft_engine::Session::chess_worksheet_count
//!
//! Once the file is written, a window says where, with what the export noted (a diagram not
//! read, a font only named) and Open folder.

use pdfcraft_engine::DocId;
use pdfcraft_engine::chess::ScanStamp;
use pdfcraft_engine::chess::export::{ChessExportOptions, ChessFormat};
use pdfcraft_engine::chess::pdfcraft_chess::describe::Naming;
use pdfcraft_engine::chess::pdfcraft_chess::san::Lang;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::LangTag;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::markdown::PgnPositions;
use pdfcraft_engine::chess::pdfcraft_chessvision::doc::worksheet::{Layout, PerPage};
use pdfcraft_engine::chess::worksheet::{Paper, SheetCount, SheetOptions};

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

/// What an export writes: the engine's formats, the book's exercises as a PGN of problems or as
/// Anki's cards (plan A9-N10, N10b), which are a PGN export to the engine, and as a sheet to print
/// (plan A9-N12b), a PDF of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ExportFormat {
    #[default]
    Pgn,
    Exercises,
    Anki,
    Sheet,
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
    pub const ALL: [ExportFormat; 9] = [
        ExportFormat::Pgn,
        ExportFormat::Exercises,
        ExportFormat::Anki,
        ExportFormat::Sheet,
        ExportFormat::Epub,
        ExportFormat::Html,
        ExportFormat::Docx,
        ExportFormat::Rtf,
        ExportFormat::Markdown,
    ];

    /// The engine's export format: the exercises are a PGN export; none for the sheet, which the
    /// engine draws apart (`Session::chess_worksheet`).
    pub fn engine(self) -> Option<ChessFormat> {
        match self {
            ExportFormat::Pgn | ExportFormat::Exercises | ExportFormat::Anki => Some(ChessFormat::Pgn),
            ExportFormat::Sheet => None,
            ExportFormat::Epub => Some(ChessFormat::Epub),
            ExportFormat::Html => Some(ChessFormat::Html),
            ExportFormat::Docx => Some(ChessFormat::Docx),
            ExportFormat::Rtf => Some(ChessFormat::Rtf),
            ExportFormat::Markdown => Some(ChessFormat::Markdown),
        }
    }

    /// The file's extension: Anki imports a text file; the sheet is a PDF.
    pub fn extension(self) -> &'static str {
        match (self, self.engine()) {
            (ExportFormat::Anki, _) => "txt",
            (_, Some(format)) => format.extension(),
            (_, None) => "pdf",
        }
    }

    /// The format's name, as the save dialog's filter shows it.
    pub fn name(self) -> &'static str {
        match (self, self.engine()) {
            (ExportFormat::Anki, _) => "Anki",
            (_, Some(format)) => format.name(),
            (_, None) => "PDF",
        }
    }

    /// The file's name the save dialog proposes, from the book's `stem`: the sheet's never the
    /// book's own (`book.pdf`), which writing over would lose.
    pub fn file_name(self, stem: &str) -> String {
        match self {
            ExportFormat::Sheet => format!("{stem} - {}.{}", tl!("exercises"), self.extension()),
            _ => format!("{stem}.{}", self.extension()),
        }
    }

    /// Whether it writes the book's exercises (plan A9-N10, N10b, N12b).
    pub fn exercises(self) -> bool {
        matches!(self, ExportFormat::Exercises | ExportFormat::Anki | ExportFormat::Sheet)
    }

    /// The format as the dialog names it: what it makes.
    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Pgn => tl!("PGN — games"),
            ExportFormat::Exercises => tl!("PGN — exercises"),
            ExportFormat::Anki => tl!("Anki — cards"),
            ExportFormat::Sheet => tl!("Exercise sheet (PDF)"),
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
            ExportFormat::Sheet => {
                Some(tl!("The book's numbered diagrams as a new PDF to print, 4, 6 or 9 a page, with their solutions at the end."))
            }
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
            ExportFormat::Sheet => "sheet",
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

/// How the exercise sheet is laid out (plan A9-N12b): 4, 6 or 9 diagrams a page, and the solutions
/// at the end or not (they are, unless the user says otherwise).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SheetChoices {
    pub per_page: PerPage,
    pub solutions: bool,
}

impl Default for SheetChoices {
    fn default() -> SheetChoices {
        SheetChoices { per_page: PerPage::default(), solutions: true }
    }
}

impl SheetChoices {
    /// The engine's options: the captions in the book's language, page by page (the dialog's
    /// language reaches them as the export's `text_lang`), on A4.
    pub fn options(self) -> SheetOptions {
        SheetOptions { layout: Layout { per_page: self.per_page, solutions: self.solutions, lang: None }, paper: Paper::A4 }
    }
}

/// The export dialog's choices that are remembered (plan A9-X2; the PGN's, P9c's open item): the
/// format, which diagrams the PGN gives a game of their own, where the PGN's games come from, how
/// the diagrams' descriptions name the pieces and how the exercise sheet is laid out (plan
/// A9-N12b).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExportChoices {
    pub format: ExportFormat,
    pub positions: PgnPositions,
    pub source: PgnSource,
    pub naming: Naming,
    pub sheet: SheetChoices,
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
        serde_json::json!({
            "format": self.format.token(),
            "positions": positions,
            "source": source,
            "descriptions": self.naming.name(),
            "per_page": self.sheet.per_page.count(),
            "solutions": self.sheet.solutions,
        })
    }

    /// Read back, each choice on its own: one this build does not know keeps its default.
    pub fn from_json(value: &serde_json::Value) -> ExportChoices {
        let text = |key: &str| value.get(key).and_then(serde_json::Value::as_str).unwrap_or_default();
        let sheet = SheetChoices::default();
        ExportChoices {
            format: ExportFormat::from_token(text("format")).unwrap_or_default(),
            positions: PgnPositions::from_name(text("positions")).unwrap_or_default(),
            source: if text("source") == "text" { PgnSource::Text } else { PgnSource::Book },
            naming: Naming::from_name(text("descriptions")).unwrap_or_default(),
            sheet: SheetChoices {
                per_page: value.get("per_page").and_then(serde_json::Value::as_u64).and_then(PerPage::of).unwrap_or(sheet.per_page),
                solutions: value.get("solutions").and_then(serde_json::Value::as_bool).unwrap_or(sheet.solutions),
            },
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

    /// The range as it reads: all of them, or its first and last page (1-based) — two ranges with
    /// the same key read the same pages.
    fn key(self, count: usize) -> (bool, usize, usize) {
        let (from, to) = self.bounds(count);
        if self.all || (from == 1 && to >= count) { (true, 1, count) } else { (false, from, to) }
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
    /// The exercise sheet's last count (plan A9-N12b).
    count: Option<Counted>,
}

/// What the exercise sheet's count found (plan A9-N12b): for which pages, in which state of the
/// document, and what it counted (`None`: it could not count, and the export will).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Counted {
    pages: (bool, usize, usize),
    stamp: Option<ScanStamp>,
    count: Option<SheetCount>,
}

/// The exercise sheet's count running off the interface's thread (plan A9-N12b): one at a time,
/// in a session of its own given what this session read, scanning nothing.
pub(crate) struct Counting {
    doc: DocId,
    pages: (bool, usize, usize),
    stamp: Option<ScanStamp>,
    job: crate::chess_job::Job<(Option<SheetCount>, pdfcraft_engine::chess::ChessCached)>,
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
        self.chess.export_dialog =
            Some(ExportDialog { doc: id, choices: self.chess.export, pages: kept.pages, lang: kept.lang, page_count, says, count: None });
    }

    /// Answer the export dialog. Its choices are kept whatever the answer but Cancel.
    pub fn answer_chess_export(&mut self, answer: DialogAnswer) {
        let Some(d) = self.chess.export_dialog.take() else { return };
        // The sheet's count is the dialog's: dropping it asks it to stop.
        self.chess.sheet_count = None;
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
        // The exercises written, and those left out and why (plan A9-N12b): a sheet's exercise
        // whose solution was not read has no moves (D30).
        if let Some(e) = made.exercises.as_deref() {
            use pdfcraft_engine::chess::pdfcraft_chessvision::doc::exercises::LeftOutReason;
            let reason = |r: LeftOutReason| e.left_out.iter().filter(|l| l.reason == r).count();
            let unsolved = e.exercises.iter().filter(|x| x.moves == 0).count();
            let found =
                SheetCount { exercises: e.exercises.len(), unsolved, no_position: reason(LeftOutReason::NoPosition), ..SheetCount::default() };
            notes.extend(exercise_parts(&found, reason(LeftOutReason::NoSolution)));
        }
        notes.extend(made.notes.iter().cloned());
        self.chess.export_done = Some(Done { path, said, notes });
    }

    /// The exercise sheet's count for dialog `d` (plan A9-N12b): pick up the one that ended, and
    /// start one for the pages chosen when none counted them in the document's state — one at a
    /// time (the newest pages are counted when it ends). Off the interface's thread, from what is
    /// already read, scanning nothing: choosing the format never reads an unread book. In the
    /// browser, where it would run inside the frame, only for as many pages as the export reads
    /// there.
    fn count_sheet(&mut self, d: &mut ExportDialog) {
        if !self.poll_sheet_count(d) {
            return;
        }
        let stamp = self.session.chess_stamp(d.doc);
        let pages = d.pages.key(d.page_count);
        if d.choices.format != ExportFormat::Sheet || d.count.is_some_and(|k| k.pages == pages && k.stamp == stamp) {
            return;
        }
        if self.chess_frame_budget().is_some() && d.pages.count(d.page_count) > crate::chess_text_ui::WEB_WHOLE_BOOK_PAGES {
            d.count = Some(Counted { pages, stamp, count: None });
            return;
        }
        // A document that can't be read here (closed meanwhile) is counted by the export: said
        // once, not tried again every frame (plan A9-N12b2).
        let Some(source) = self.chess_job_source(d.doc) else {
            d.count = Some(Counted { pages, stamp, count: None });
            return;
        };
        let mut options = self.chess_export_base(d.doc, ExportFormat::Sheet, "");
        // The dialog's pages, not those it was last answered with.
        options.pages = d.pages.pages(d.page_count);
        let games = self.export_games(d.doc);
        let layout = d.choices.sheet.options().layout;
        let job = crate::chess_job::Job::spawn("pdfcraft-chess-sheet-count", self.run_inline, move |_| {
            let (mut s, id) = source.open()?;
            crate::chess_text_ui::link_games(&mut options, games.as_deref());
            let count = s.chess_worksheet_count(id, &options, &layout).ok();
            Ok((count, s.chess_cached(id, &[])))
        });
        self.chess.sheet_count = Some(Counting { doc: d.doc, pages, stamp, job });
        // Inline (tests, the web) it has ended already.
        self.poll_sheet_count(d);
    }

    /// Pick up the exercise sheet's count when it ended, into dialog `d` when it counted the
    /// dialog's document in the state it is in. Whether none is running now.
    fn poll_sheet_count(&mut self, d: &mut ExportDialog) -> bool {
        let Some(c) = self.chess.sheet_count.as_mut() else { return true };
        let Some(out) = c.job.poll() else {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
            return false;
        };
        let Some(c) = self.chess.sheet_count.take() else { return true };
        let (count, cached) = match out {
            Ok((count, cached)) => (count, Some(cached)),
            Err(_) => (None, None),
        };
        let stamp = self.session.chess_stamp(c.doc);
        // What it read is this session's too, while the document is in the state it read.
        if let Some(cached) = cached.filter(|_| c.stamp.is_some() && c.stamp == stamp) {
            self.session.prime_chess_cache(c.doc, cached);
        }
        if c.doc == d.doc && c.stamp == stamp {
            d.count = Some(Counted { pages: c.pages, stamp: c.stamp, count });
        }
        true
    }

    /// The exercise sheet's count of dialog `d`, when it counted the pages chosen in the
    /// document's state now: `Some(None)` when it could not.
    fn sheet_counted(&self, d: &ExportDialog) -> Option<Option<SheetCount>> {
        let stamp = self.session.chess_stamp(d.doc);
        d.count.filter(|k| k.pages == d.pages.key(d.page_count) && k.stamp == stamp).map(|k| k.count)
    }

    /// Why the exercise sheet of dialog `d` is refused (plan A9-N12b; the engine refuses it too):
    /// every page chosen is read and none holds an exercise — and the pages before them the
    /// export looks at for a caption were looked at too (plan A9-N12b2). Otherwise the export may
    /// find a number the count could not, and only it refuses.
    fn sheet_refused(&self, d: &ExportDialog) -> Option<String> {
        if d.choices.format != ExportFormat::Sheet {
            return None;
        }
        let c = self.sheet_counted(d).flatten().filter(|c| c.pages_not_read == 0 && c.exercises == 0 && c.before_looked_at)?;
        Some(if c.no_position > 0 {
            crate::i18n::fmt(
                tl!("No exercise to put on the sheet: the numbered diagrams of the pages chosen have no position to start from ({n})."),
                &[("n", &c.no_position.to_string())],
            )
        } else {
            tl!("No exercise to put on the sheet: the pages chosen have no numbered diagram.").to_owned()
        })
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
        if format == ExportFormat::Sheet {
            // The exercises of the pages already read (plan A9-N12b); the rest the export counts.
            match self.sheet_counted(d) {
                None => parts.push(tl!("Exercises: counting…").to_owned()),
                Some(None) => parts.push(tl!("Exercises: counted when exported").to_owned()),
                Some(Some(c)) => {
                    if c.pages_counted > 0 {
                        parts.extend(exercise_parts(&c, 0));
                    }
                    if c.pages_not_read > 0 {
                        parts.push(crate::i18n::fmt(tl!("Pages not read yet: {n} (counted when exported)"), &[("n", &c.pages_not_read.to_string())]));
                    }
                }
            }
        }
        if !format.exercises() {
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

/// What the exercises of `c` come to (plan A9-N12b), for the dialog's summary and the window the
/// export ends with: how many, those whose solution was not read (on the sheet all the same,
/// D30), and those left out and why — no position to start from, `no_solution` (the PGN of
/// exercises and Anki's cards leave them out), past the sheet's cap.
fn exercise_parts(c: &SheetCount, no_solution: usize) -> Vec<String> {
    let n = |v: usize| v.to_string();
    let mut said = crate::i18n::fmt(tl!("Exercises: {n}"), &[("n", &n(c.exercises))]);
    if c.unsolved > 0 {
        said = format!("{said} ({})", crate::i18n::fmt(tl!("solution not read: {n}"), &[("n", &n(c.unsolved))]));
    }
    let mut parts = vec![said];
    for (left, why) in [(c.no_position, tl!("no position to start from")), (no_solution, tl!("no solution read"))] {
        if left > 0 {
            parts.push(crate::i18n::fmt(tl!("Left out: {n} ({why})"), &[("n", &n(left)), ("why", why)]));
        }
    }
    if c.over > 0 {
        let max = n(pdfcraft_engine::chess::pdfcraft_chessvision::doc::worksheet::MAX_EXERCISES);
        let why = crate::i18n::fmt(tl!("past the sheet's {max}"), &[("max", &max)]);
        parts.push(crate::i18n::fmt(tl!("Left out: {n} ({why})"), &[("n", &n(c.over)), ("why", &why)]));
    }
    parts
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
    if format == ExportFormat::Sheet {
        row(ui, tl!("Per page"), |ui, _| {
            ui.horizontal(|ui| {
                ui.scope(|ui| {
                    crate::widgets::radio_group(ui, tl!("Per page"));
                    ui.horizontal(|ui| {
                        for (per_page, grid) in [(PerPage::Four, "2 × 2"), (PerPage::Six, "2 × 3"), (PerPage::Nine, "3 × 3")] {
                            let tip = crate::i18n::fmt(tl!("Diagrams a page: {n} ({grid})"), &[("n", &per_page.count().to_string()), ("grid", grid)]);
                            crate::widgets::radio_value(ui, &mut d.choices.sheet.per_page, per_page, per_page.count().to_string()).on_hover_text(tip);
                        }
                    });
                });
                ui.add_space(16.0);
                ui.add(crate::widgets::checkbox(&mut d.choices.sheet.solutions, tl!("With the solutions")))
                    .on_hover_text(tl!("The solutions at the end, in the order of the numbers. A diagram whose solution was not read says so."));
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
        app.count_sheet(&mut d);
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
            let refused = app.sheet_refused(&d);
            if let Some(why) = &refused {
                ui.add_space(6.0);
                crate::notice::callout(ui, crate::notice::Tone::Warning, why);
            }
            if to_review > 0 {
                ui.add_space(6.0);
                if pending(ui, &t, to_review) {
                    answer = Some(DialogAnswer::ReviewFirst);
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add_enabled_ui(refused.is_none(), |ui| crate::widgets::pill_button(ui, tl!("Export…"), true)).inner.clicked() {
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
        let c = ExportChoices {
            format: ExportFormat::Anki,
            positions: PgnPositions::None,
            source: PgnSource::Text,
            naming: Naming::Words,
            sheet: SheetChoices { per_page: PerPage::Six, solutions: false },
        };
        assert_eq!(ExportChoices::from_json(&c.to_json()), c);
        assert_eq!(ExportChoices::from_json(&serde_json::json!({ "format": "tiff", "positions": 3 })), ExportChoices::default());
        assert_eq!(ExportChoices::from_json(&serde_json::Value::Null), ExportChoices::default());
        for f in ExportFormat::ALL {
            assert_eq!(ExportFormat::from_token(f.token()), Some(f));
        }
        assert_eq!(ExportFormat::Anki.extension(), "txt");
        assert_eq!(ExportFormat::Exercises.engine(), Some(ChessFormat::Pgn));
        assert_eq!((ExportFormat::Sheet.engine(), ExportFormat::Sheet.extension()), (None, "pdf"));
        assert_ne!(ExportFormat::Sheet.file_name("book"), "book.pdf", "never the book's own name");
        // The sheet's layout: its per page and its solutions, and a count this build does not know.
        let sheet = ExportChoices { sheet: SheetChoices { per_page: PerPage::Nine, solutions: false }, ..ExportChoices::default() };
        assert_eq!(ExportChoices::from_json(&sheet.to_json()), sheet);
        assert_eq!(ExportChoices::from_json(&serde_json::json!({ "per_page": 5 })).sheet, SheetChoices::default());
        assert!(SheetChoices::default().solutions, "with the solutions unless the user says otherwise");
    }

    /// The export dialog on document `doc`, the exercise sheet chosen, page 2 of 3 (1-based).
    fn sheet_dialog(doc: DocId) -> ExportDialog {
        let choices = ExportChoices { format: ExportFormat::Sheet, ..ExportChoices::default() };
        ExportDialog { doc, choices, pages: Pages { all: false, from: 2, to: 2 }, lang: None, page_count: 3, says: BookSays::FromText, count: None }
    }

    /// A blank PDF of `n` pages.
    fn blank_pdf(n: usize) -> Vec<u8> {
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i)).collect();
        let mut objs = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
            format!("<< /Type /Pages /Kids [{}] /Count {n} /MediaBox [0 0 200 300] >>", kids.join(" ")),
        ];
        objs.extend((0..n).map(|_| "<< /Type /Page /Parent 2 0 R >>".to_owned()));
        let mut out = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            offsets.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n{o}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for o in offsets {
            out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    /// Plan A9-N12b2 (the review of #406): every page chosen read and no exercise found refuses
    /// the sheet in the dialog only when the page before, which the export looks at for a caption
    /// at its foot, was looked at by the count too; otherwise the export says whether there is one.
    #[test]
    fn the_sheet_is_refused_only_when_the_page_before_was_looked_at() {
        let app = PdfCraftApp::new();
        let mut d = sheet_dialog(DocId(9));
        let pages = d.pages.key(d.page_count);
        let none = SheetCount { pages_counted: 1, ..SheetCount::default() };
        d.count = Some(Counted { pages, stamp: None, count: Some(none) });
        assert_eq!(app.sheet_refused(&d), None, "the page before not looked at: left to the export");
        d.count = Some(Counted { pages, stamp: None, count: Some(SheetCount { before_looked_at: true, ..none }) });
        assert!(app.sheet_refused(&d).is_some_and(|why| why.contains("no numbered diagram")), "{:?}", app.sheet_refused(&d));
    }

    /// Plan A9-N12b2: a dialog whose document can't be read for a count (closed meanwhile) is
    /// counted by the export, said so once; it is not tried again every frame ("counting…" for
    /// ever).
    #[test]
    fn a_document_that_cannot_be_counted_is_left_to_the_export() {
        let mut app = PdfCraftApp::new();
        app.run_inline = true;
        let mut d = sheet_dialog(DocId(9));
        app.count_sheet(&mut d);
        assert!(app.chess.sheet_count.is_none(), "nothing runs");
        assert_eq!(app.sheet_counted(&d), Some(None), "it could not count: the export will");
        app.count_sheet(&mut d);
        assert_eq!(app.sheet_counted(&d), Some(None), "and it is not tried again");
    }

    /// Plan A9-N12b2: the pages are changed while a count runs. The count that ends is of the pages
    /// it was started for: the dialog never takes it for the new ones ("counting…" until they are
    /// counted), and the next frame counts them.
    #[test]
    fn a_count_of_pages_since_changed_is_not_taken_for_the_new_ones() {
        let (none, _) = pdfcraft_engine::chess::pdfcraft_chessvision::models::ChessModels::from_files(Vec::new());
        pdfcraft_engine::chess::install_models(Some(none));
        let mut app = PdfCraftApp::new();
        app.run_inline = true;
        app.open_bytes("book.pdf", None, blank_pdf(3)).expect("opens");
        let doc = app.views[0].id;
        let mut d = sheet_dialog(doc);
        let started_for = d.pages.key(d.page_count);
        let stamp = app.session.chess_stamp(doc);
        let (go, gate) = std::sync::mpsc::channel::<()>();
        let stale = SheetCount { exercises: 5, pages_counted: 1, before_looked_at: true, ..SheetCount::default() };
        let job = crate::chess_job::Job::spawn("pdfcraft-chess-sheet-count-test", false, move |_| {
            gate.recv().map_err(|e| e.to_string())?;
            Ok((Some(stale), pdfcraft_engine::chess::ChessCached::default()))
        });
        app.chess.sheet_count = Some(Counting { doc, pages: started_for, stamp, job });
        // Page 3 chosen while page 2 is counted.
        d.pages = Pages { all: false, from: 3, to: 3 };
        app.count_sheet(&mut d);
        assert_eq!(app.sheet_counted(&d), None, "counting…");
        go.send(()).expect("the count waits");
        assert!(app.chess.sheet_count.as_ref().is_some_and(|c| c.job.wait(std::time::Duration::from_secs(60))), "the count ended");
        app.count_sheet(&mut d);
        assert!(app.chess.sheet_count.is_none(), "the new pages counted, here (inline)");
        assert_eq!(app.sheet_counted(&d), Some(Some(SheetCount { pages_not_read: 1, ..SheetCount::default() })), "page 3's count, not page 2's");
    }
}
