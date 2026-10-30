//! The tool table: names, descriptions and JSON Schemas for every automation tool.
//!
//! Tool names use `[a-z_]` only (MCP clients reject dots). `command` links a tool to the
//! registry id it automates, so `command_list` can tell an agent which tool runs a menu command.

use serde_json::{Value, json};

use crate::ToolError;

#[derive(Clone, Debug)]
pub struct ToolDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    /// JSON Schema (draft 2020-12 subset) for the arguments object.
    pub input_schema: Value,
    /// Does not change any document or file.
    pub read_only: bool,
    /// May remove content or overwrite files.
    pub destructive: bool,
    /// The registry command this tool automates, if any.
    pub command: Option<&'static str>,
}

/// What a tool's `pieces` takes: installed, geometric, or a set PdfCraft ships by its id.
fn pieces_values() -> Vec<&'static str> {
    let shipped = pdfcraft_engine::chess::pdfcraft_chessvision::pieces::bundled::SETS.iter().map(|s| s.id);
    ["installed", "geometric"].into_iter().chain(shipped).collect()
}

fn doc() -> Value {
    json!({ "type": "integer", "minimum": 1, "description": "Document id from doc_open or doc_list." })
}

fn pages(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "integer", "minimum": 1 }, "minItems": 1, "description": format!("1-based page numbers {what}.") })
}

fn path(what: &str) -> Value {
    json!({ "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": format!("{what}: 1-based positions from the top level, e.g. [2, 1] = the first child of the second bookmark.") })
}

fn path_arg() -> Value {
    json!({ "type": "string", "description": "A file path (relative to --root when set)." })
}

fn point() -> Value {
    json!({ "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2, "description": "[x, y] in points from the top-left of the displayed page." })
}

fn measure_points() -> Value {
    json!({"type":"array","items":point(),"minItems":1,"maxItems":2048})
}
fn measure_schema() -> Value {
    schema(
        json!({"doc":doc(),"page":{"type":"integer","minimum":1},"points":measure_points(),"label":{"type":"string","maxLength":512},"author":{"type":"string","maxLength":512}}),
        &["doc", "page", "points"],
    )
}

fn color() -> Value {
    json!({ "type": "string", "description": "#RRGGBB or a name: yellow, red, orange, green, blue, purple, pink, black, gray, white." })
}

/// Properties that pick one comment: its `id` (from comment_list), or `page` + `index`.
fn comment_ref(mut extra: Value) -> Value {
    if let Some(props) = extra.as_object_mut() {
        props.insert("doc".into(), doc());
        props.insert("id".into(), json!({ "type": "string", "description": "The comment's id (from comment_list)." }));
        props.insert("page".into(), json!({ "type": "integer", "minimum": 1, "description": "With `index`, instead of `id`." }));
        props.insert(
            "index".into(),
            json!({ "type": "integer", "minimum": 1, "description": "1-based position on the page, as comment_list reports." }),
        );
    }
    extra
}

fn models_dir() -> Value {
    json!({ "type": "string", "description": "A file path to the folder holding chess-pieces.onnx/.json (relative to --root when set). Default: PDFCRAFT_MODELS, then beside the executable." })
}

/// The arguments of `chess_detect` and, with `read`, `chess_read`.
fn chess_props(read: bool) -> Value {
    let mut props = json!({
        "doc": doc(),
        "pages": pages("to scan (default: all)"),
        "dpi": { "type": "number", "minimum": 72, "maximum": 600, "description": "Resolution pages are rendered at to find diagrams (default 220)." },
        "order": { "type": "string", "enum": ["column", "row"], "description": "Reading order of the diagrams on a page (default column)." },
        "max_per_page": { "type": "integer", "minimum": 1, "maximum": 64, "description": "Most diagrams kept per page, in reading order (default 64)." },
        "allow_unverified_fonts": { "type": "boolean", "description": "Also read chess fonts whose layout was not checked (default false)." },
    });
    if read && let Some(o) = props.as_object_mut() {
        o.insert("models_dir".into(), models_dir());
        o.insert(
            "max_read_dpi".into(),
            json!({ "type": "number", "minimum": 72, "maximum": 600, "description": "Highest resolution a page is rendered at again for reading, when a diagram (an embedded image) has finer detail than dpi (default 600)." }),
        );
    }
    props
}

/// The diagram corrections a reading tool takes from the corrections folder (plan A8, P3a3).
fn insert_corrections(props: &mut Value) {
    if let Some(o) = props.as_object_mut() {
        o.insert("corrections".into(), json!({ "type": "boolean", "description": "Use the diagram corrections kept for doc in store (default true): a corrected diagram takes the position a person set, whatever it reads as now (chess_correction_set). The reply's corrections says which file was read (kept: how many), each applied (uid, page, index, kind, fen, read_fen: what it reads as now, diverged: the reading changed since it was corrected) and each unmatched (its page was read but its diagram was not found there, and why)." }));
        o.insert("store".into(), json!({ "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." }));
    }
}

/// The arguments of `chess_replace_diagram`: the diagram as `chess_detect` numbered it (and the
/// scan settings it used), the position and how to draw it.
fn chess_replace_props() -> Value {
    let mut props = chess_props(false);
    if let Some(o) = props.as_object_mut() {
        o.remove("pages");
        let extra = json!({
            "page": { "type": "integer", "minimum": 1, "description": "The diagram's page." },
            "index": { "type": "integer", "minimum": 1, "description": "The diagram's number on the page, as chess_detect or chess_read numbered it (with the same dpi, order, max_per_page and allow_unverified_fonts)." },
            "generation": { "type": "integer", "minimum": 0, "description": "The generation chess_detect or chess_read reported. Refused when the document changed since." },
            "fen": { "type": "string", "maxLength": 128, "description": "The position to draw, as a FEN (only its placement is drawn). Give fen or placement; with neither, the position kept for the diagram in store (chess_correction_set) is drawn." },
            "placement": { "type": "string", "maxLength": 100, "description": "The position to draw, as the first field of a FEN." },
            "models_dir": models_dir(),
            "keep_correction": { "type": "boolean", "description": "Keep the position drawn as the diagram's correction in store, with the page as it is after the drawing, as the app's Redraw in the PDF does (default false; needs fen or a kept correction, not placement)." },
            "store": { "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." },
            "light": color(),
            "dark": color(),
            "border": { "type": "number", "minimum": 0, "maximum": 1, "description": "Frame width in square widths (default 0.04; 0 for none)." },
            "border_color": color(),
            "coordinates": { "type": "boolean", "description": "Print a-h and 1-8 around the board (default false)." },
            "flipped": { "type": "boolean", "description": "Draw from Black's side (default false)." },
            "pieces": { "type": "string", "enum": pieces_values(), "description": "installed (default): PDFCRAFT_CHESS_PIECES (a font or a folder of piece images), a diagram font in the fonts folder beside the executable, an installed Chess Merida or a pieces folder beside the executable, else plain geometric pieces; geometric: always the plain pieces; any other value is a set PdfCraft ships, by id (chessnut…)." },
            "board_theme": board_theme(),
            "drawings": { "type": "string", "maxLength": 2000, "description": "Arrows and rings to draw over the board, as PGN comment commands: [%cal Gb5c7,Rd1h5] for arrows, [%csl Re4] for rings, colours G, R, Y or B. The result describes them in words." },
            "dry_run": { "type": "boolean", "description": "Only say what would be done; change nothing (default false)." },
        });
        if let Value::Object(extra) = extra {
            o.extend(extra);
        }
    }
    props
}

/// The arguments of `chess_export_html` and `chess_export_epub`.
/// A board theme's id, for the squares of drawn diagrams (plan HC7).
fn board_theme() -> Value {
    let ids: Vec<&str> = pdfcraft_engine::chess::pdfcraft_chess::theme::THEMES.iter().map(|t| t.id).collect();
    json!({ "type": "string", "enum": ids, "description": "The squares' colours of the drawn diagrams, as a board theme (default brown, the boards' colours; book hatches the dark squares, as printed diagrams). Wood, marble and paper draw flat in their colours. light and dark, where taken, override it." })
}

fn chess_export_schema() -> Value {
    schema(
        {
            let mut props = chess_props(true);
            if let Some(o) = props.as_object_mut() {
                o.insert("out".into(), json!({ "type": "string", "description": "The file to write." }));
                o.insert("board_theme".into(), board_theme());
                o.insert(
                    "review_marks".into(),
                    json!({ "type": "boolean", "description": "HTML and EPUB: highlight what the reading flagged for review (default: on in HTML, off in EPUB, a book for readers)." }),
                );
                o.insert("diagram_px".into(), json!({ "type": "integer", "minimum": 32, "maximum": 2400, "description": "Word, RTF and Markdown: the diagram images' side in pixels (default 900: 300 ppi at the 3 inches Word and RTF set a diagram at)." }));
                o.insert(
                    "title".into(),
                    json!({ "type": "string", "maxLength": 200, "description": "The page title (default: the document's name)." }),
                );
                o.insert("lang".into(), json!({ "type": "string", "maxLength": 16, "description": "The piece letters' language in the PDF, as a BCP 47 tag. Default: chosen from the pages." }));
                o.insert("ocr".into(), json!({ "type": "string", "enum": ["auto", "off", "all"], "description": "Chess OCR of the page image instead of the text layer: auto (default) on pages whose layer is poor or missing, or is another program's invisible OCR over a scan; off (a born-digital book: never, poor pages are listed in needs_ocr); or all pages (a scanned book). Needs the user's chess-glyphs model; the general OCR reads prose when its models are installed. ocr_pages says what it read." }));
                o.insert("cipher_in".into(), json!({ "type": "string", "description": "A book cipher saved by an earlier run (cipher_out), to start chess OCR from: what the book's figurine fonts or misreadings map to, learned on other pages." }));
                o.insert(
                    "cipher_out".into(),
                    json!({ "type": "string", "description": "Save the book cipher these pages taught (JSON), for later runs on the same book." }),
                );
                o.insert(
                    "diagrams".into(),
                    json!({ "type": "string", "enum": ["svg", "fen"], "description": "Diagrams as redrawn SVG boards (default) or as FEN text." }),
                );
                o.insert("figurines".into(), json!({ "type": "string", "enum": ["unicode", "letters", "printed"], "description": "Pieces in moves as Unicode figurines (default), letters, or as printed." }));
                o.insert("letters_lang".into(), json!({ "type": "string", "maxLength": 16, "description": "With figurines: letters, the language of the letters (default: lang, else en)." }));
                o.insert(
                    "links".into(),
                    json!({ "type": "boolean", "description": "A lichess link under each diagram that has one (default true)." }),
                );
                o.insert(
                    "link_text".into(),
                    json!({ "type": "string", "maxLength": 80, "description": "The links' text (default \"Analyze on lichess\")." }),
                );
                o.insert("pieces".into(), json!({ "type": "string", "enum": pieces_values(), "description": "installed (default): PDFCRAFT_CHESS_PIECES (a font or a folder of piece images), a diagram font in the fonts folder beside the executable, an installed Chess Merida or a pieces folder beside the executable, else geometric; geometric: always the plain pieces; any other value is a set PdfCraft ships, by id (chessnut…)." }));
            }
            insert_corrections(&mut props);
            props
        },
        &["doc", "out"],
    )
}

/// The PGN writers' options (plan A8 V4), by name.
fn pgn_style_props() -> [(&'static str, Value); 3] {
    [
        (
            "variations",
            json!({ "type": "string", "maxLength": 32, "description": "Which variations the PGN keeps: all (default), main (the main line alone) or depth:N (variations nested at most N deep: depth:1 keeps the main line's own variations)." }),
        ),
        (
            "comments",
            json!({ "type": "string", "enum": ["all", "none"], "description": "all (default) or none: no comments, nor the arrows and coloured squares written in them." }),
        ),
        ("nags", json!({ "type": "string", "enum": ["all", "none"], "description": "all (default) or none: no annotation glyphs ($1, $220…)." })),
    ]
}

/// [`chess_export_schema`] and the PGN's own options.
fn chess_export_pgn_schema() -> Value {
    let mut s = chess_export_schema();
    if let Some(props) = s.get_mut("properties").and_then(Value::as_object_mut) {
        for (name, prop) in pgn_style_props() {
            props.insert(name.into(), prop);
        }
        props.insert(
            "positions".into(),
            json!({ "type": "string", "enum": ["unused", "all", "none"], "description": "Which diagrams get a game of their own with no moves: unused (default) those no game starts from or reprints, all every legal one, none only the games." }),
        );
    }
    s
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

struct T {
    name: &'static str,
    title: &'static str,
    description: &'static str,
    read_only: bool,
    destructive: bool,
    command: Option<&'static str>,
}

const fn t(name: &'static str, title: &'static str, description: &'static str) -> T {
    T { name, title, description, read_only: false, destructive: false, command: None }
}

impl T {
    const fn ro(mut self) -> Self {
        self.read_only = true;
        self
    }
    const fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }
    const fn cmd(mut self, id: &'static str) -> Self {
        self.command = Some(id);
        self
    }
    fn with(self, input_schema: Value) -> ToolDef {
        ToolDef {
            name: self.name,
            title: self.title,
            description: self.description,
            input_schema,
            read_only: self.read_only,
            destructive: self.destructive,
            command: self.command,
        }
    }
}

/// Every tool, in a stable order.
pub fn tools() -> Vec<ToolDef> {
    let save_out = json!({ "type": "string", "description": "File to write. Omit to open the result as a new unsaved document instead." });
    let open = json!({ "type": "boolean", "description": "Also open the result as a new document (default: only when out is omitted)." });
    vec![
        t("doc_batch", "Process files and folders", "Run convert, ocr and/or optimize over files and folders, one file at a time. recursive defaults to false. Preserve paths relative to each selected folder inside out_dir; duplicate inputs run once. Skip unsupported files, links and the output subtree. Existing outputs receive numbered names and are never replaced. Continue after per-file errors unless stop_on_error is true. Return completed/failed/skipped counts and per-file results. Discovery is bounded to 10000 entries and 64 folder levels. Convert must be first; each step can appear once. OCR uses installed models. Optimize uses Reduce File Size settings, optionally overriding target ppi (threshold 1.5 times ppi) and JPEG quality; a larger candidate keeps the original bytes. Sources and open documents are unchanged.")
            .with(schema(json!({
                "paths": {"type":"array","items":{"type":"string"},"minItems":1,"maxItems":1000},
                "out_dir": {"type":"string"},
                "steps": {"type":"array","items":{"type":"string","enum":["convert","ocr","optimize"]},"minItems":1,"maxItems":3,"uniqueItems":true},
                "recursive": {"type":"boolean"}, "stop_on_error": {"type":"boolean"},
                "ppi": {"type":"number","minimum":9,"maximum":1600},
                "quality": {"type":"integer","minimum":1,"maximum":100},
                "dpi": {"type":"number","minimum":72,"maximum":600},
                "language": {"type":"string"}, "skip_text_pages": {"type":"boolean"}
            }), &["paths","out_dir","steps"])),
        t("doc_open", "Open a PDF", "Open a PDF file and return its document id, page count and whether it can be edited.")
            .cmd("file.open")
            .with(schema(json!({ "path": { "type": "string" }, "password": { "type": "string", "description": "User or owner password for encrypted files." } }), &["path"])),
        t("doc_list", "List open documents", "List the open documents with their ids, page counts and unsaved state.").ro().with(schema(json!({}), &[])),
        t("doc_info", "Inspect a document", "Metadata, page sizes and labels, bookmarks, annotations, form fields, links, layers, page transitions and display times, attachments, fonts, security and repair notes.")
            .ro()
            .cmd("file.properties")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("doc_close", "Close a document", "Close a document. Fails if it has unsaved changes unless discard_changes is true.")
            .cmd("file.close")
            .with(schema(json!({ "doc": doc(), "discard_changes": { "type": "boolean" } }), &["doc"])),
        t(
            "doc_save",
            "Save a document",
            "Save to its own file (an incremental update, which keeps signatures valid) or to a new path (a full rewrite). The write is atomic. flatten_fill_sign bakes Fill & Sign text, marks and signatures into the page first and removes those annotations; other comments and form fields stay. Omit it, or pass false, to leave them editable.",
        )
        .destructive()
        .cmd("file.save")
        .with(schema(
            json!({
                "doc": doc(),
                "path": { "type": "string", "description": "Save as this file. Omit to save in place." },
                "full": { "type": "boolean", "description": "Force a full rewrite (or, with false, an incremental update)." },
                "flatten_fill_sign": { "type": "boolean", "description": "Bake Fill & Sign marks into the page before writing. Default false." }
            }),
            &["doc"],
        )),
        t("doc_set_info", "Set document metadata", "Set a document information entry such as Title, Author, Subject or Keywords, or a custom property under any other name (no spaces, delimiters, / or #; at most 127 bytes; doc_info lists them under custom); an empty value removes it. When the file has XMP metadata, the matching XMP property (dc:title, dc:creator, dc:description, pdf:Keywords, xmp:CreatorTool, pdf:Producer) is changed too, so viewers that read XMP first show the new value. Undoable.")
            .with(schema(json!({ "doc": doc(), "key": { "type": "string" }, "value": { "type": "string" } }), &["doc", "key", "value"])),
        t("page_render", "Render a page", "Render one page to a PNG image (default 96 dpi, at most 600).")
            .ro()
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "dpi": { "type": "number", "minimum": 1, "maximum": 600 },
                "hide_text_block": { "type": "integer", "minimum": 1, "description": "Render an inline editor background without this paragraph (index from text_paragraphs). Does not modify the document." },
                "hide_added_text": { "type": "integer", "minimum": 1, "description": "Render a retyping background without this added text item (added index from object_list). Mutually exclusive with hide_text_block. Does not modify the document." }
            }), &["doc", "page"])),
        t("object_list", "List editable objects", "List existing paragraphs, Image/Form artwork and added content once, with source kind/index references, displayed rectangles and document generation. Indexes change when content changes; use generation in object_move to reject stale references.")
            .ro().with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1}}), &["doc","page"])),
        t("object_delete", "Delete objects", "Edit text & images ▸ Delete: remove 1–1000 mixed references from object_list together. A paragraph's lines are taken out of their text with the text around them in place, an image's or Form's drawing is removed, added content goes with its stream. Success is one undo step; a bad or stale reference (generation) refuses the whole deletion.")
            .destructive()
            .with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},
                "objects":{"type":"array","minItems":1,"maxItems":1000,"uniqueItems":true,"items":{
                    "type":"object","properties":{"kind":{"type":"string","enum":["added","text","image"]},"index":{"type":"integer","minimum":1}},"required":["kind","index"],"additionalProperties":false}},
                "generation":{"type":"integer","minimum":0}}), &["doc","page","objects"])),
        t("object_move", "Move objects together", "Atomically translate 1–1000 mixed references from object_list by [dx, dy] in displayed points (right/down). Preserves glyph codes, fonts, image data and relative spacing; success is one undo step. Clipping and vertical text are refused. A bad/stale reference refuses the entire move.")
            .with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},
                "objects":{"type":"array","minItems":1,"maxItems":1000,"uniqueItems":true,"items":{
                    "type":"object","properties":{"kind":{"type":"string","enum":["added","text","image"]},"index":{"type":"integer","minimum":1}},"required":["kind","index"],"additionalProperties":false}},
                "offset":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2},
                "generation":{"type":"integer","minimum":0,"description":"Use generation returned by object_list to reject a stale selection."}}), &["doc","page","objects","offset"])),
        t("text_word_count", "Word count", "Count Unicode words and characters on selected pages (default all), with totals and per-page counts. Table/code exclusions use PDF structure tags, not visual guesses; unclassified text is retained. Image exclusion removes Figure-tagged and invisible/OCR text. Does not run OCR. Copy permission is required. Fails rather than returning a partial total when a page cannot be read.")
            .ro()
            .cmd("file.word_count")
            .with(schema(json!({ "doc": doc(), "pages": pages("to count (default: all)"),
                "exclude_tables": { "type": "boolean" }, "exclude_code": { "type": "boolean" }, "exclude_images": { "type": "boolean" }
            }), &["doc"])),
        t("text_extract", "Extract text", "Extract the text of some or all pages, in reading order. With rect, only the text inside that rectangle on each page, as Column select takes it: one row per visual line, side-by-side pieces (table cells) separated by a tab.")
            .ro()
            .cmd("edit.column_select")
            .with(schema(
                json!({
                    "doc": doc(), "pages": pages("to extract (default: all)"),
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "Column select: [x0, y0, x1, y1] in points from the top-left of the displayed page; a glyph counts when its centre is inside." }
                }),
                &["doc"],
            )),
        t("text_find", "Find text", "Find a phrase (case-insensitive, whitespace-normalised) and return matches in page order with page and line rectangles in points (origin top-left). Reads growing batches, looking one match past the limit for a continuation. Failed pages do not discard readable matches: warnings contains up to 100 {page, error, error_truncated} entries (errors capped at 1024 UTF-8 bytes), warning_count counts failed pages, and warnings_truncated reports a capped list. complete is false after a page failure or when more matches exist; next is present only for a match-limit continuation.")
            .ro()
            .cmd("edit.find")
            .with(schema(json!({ "doc": doc(), "query": { "type": "string", "minLength": 1 }, "limit": { "type": "integer", "minimum": 1, "description": "Maximum matches (default 500). When another match exists, next holds start_page and skip to continue. complete is false for either additional matches or failed pages; an exhausted search with warnings has no next." },
                "start_page": { "type": "integer", "minimum": 1, "description": "Page to start searching from (default 1)." },
                "skip": { "type": "integer", "minimum": 0, "description": "Matches to pass over on start_page before returning any (default 0)." } }), &["doc", "query"])),
        t("page_rotate", "Rotate pages", "Rotate pages by a multiple of 90 degrees (positive is clockwise): the listed pages (default all), filtered like Acrobat's Rotate Pages by subset (all, even, odd page numbers) and orientation (all, landscape, portrait). Undoable.")
            .cmd("page.rotate")
            .with(schema(
                json!({
                    "doc": doc(), "pages": pages("to rotate (default: all)"), "degrees": { "type": "integer" },
                    "subset": { "type": "string", "enum": ["all", "even", "odd"] },
                    "orientation": { "type": "string", "enum": ["all", "landscape", "portrait"] },
                }),
                &["doc", "degrees"],
            )),
        t("page_enhance_camera", "Enhance camera image", "Straighten a photographed document (Scan & OCR ▸ Enhance ▸ Camera image): the part of the page inside the four corners (x1, y1, … x4, y4 in top-left-origin points, any order, a convex quadrilateral; when omitted, the document's edges are found automatically, or an error says they could not be) is warped to an upright rectangle that replaces what the page shows, at the page picture's own resolution (at most 600 dpi). mode is how it looks: original, colour (default), greyscale or bw (black and white, stored losslessly). level (0–100, default 0) then cleans the photo up: even lighting so the paper turns white (casts, shadows, dark edges), more contrast, and from 60 no isolated specks; colour ink stays colour. For bw the level is the threshold (higher keeps fainter strokes black). The page's comments, links and bookmarks stay. Undoable.")
            .cmd("ocr.enhance_camera")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "corners": { "type": "array", "items": { "type": "number" }, "minItems": 8, "maxItems": 8 },
                    "level": { "type": "integer", "minimum": 0, "maximum": 100 },
                    "mode": { "type": "string", "enum": ["original", "colour", "greyscale", "bw"] },
                }),
                &["doc", "page"],
            )),
        t("page_delete", "Delete pages", "Delete pages. Undoable until saved.")
            .destructive()
            .cmd("page.delete")
            .with(schema(json!({ "doc": doc(), "pages": pages("to delete") }), &["doc", "pages"])),
        t("page_move", "Move pages", "Move pages so the first of them lands at position `to` (1-based, counted before the move). Undoable.").with(schema(
            json!({ "doc": doc(), "pages": pages("to move"), "to": { "type": "integer", "minimum": 1 } }),
            &["doc", "pages", "to"],
        )),
        t("page_reverse", "Reverse pages", "Reverse the order of pages among the positions they occupy: the listed pages (at least two), or the whole document when pages is omitted. Undoable.")
            .cmd("page.reverse")
            .with(schema(json!({ "doc": doc(), "pages": pages("to reverse (default: all)") }), &["doc"])),
        t("page_insert_blank", "Insert a blank page", "Insert a blank page so it becomes page `at`. Size defaults to the neighbouring page. Undoable.")
            .cmd("page.insert_blank")
            .with(schema(
                json!({ "doc": doc(), "at": { "type": "integer", "minimum": 1 }, "width": { "type": "number", "description": "Points." }, "height": { "type": "number", "description": "Points." } }),
                &["doc", "at"],
            )),
        t("page_insert_file", "Insert pages from a file", "Insert pages of another PDF so the first becomes page `at`. Its bookmarks come along, collapsed under one bookmark named after the file, placed among the document's bookmarks where the pages went. Undoable.")
            .cmd("page.insert")
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" }, "at": { "type": "integer", "minimum": 1 }, "pages": pages("of the source file (default: all)") }), &["doc", "path", "at"])),
        t("page_extract", "Extract pages", "Copy pages into a new PDF (links, bookmarks, fields and layers that belong to them come along). separate: true writes each page as its own file into out_dir; delete: true removes the pages from this document afterwards (undoable).")
            .cmd("page.extract")
            .with(schema(
                json!({ "doc": doc(), "pages": pages("to extract"), "out": save_out.clone(), "open": open.clone(), "separate": { "type": "boolean" }, "out_dir": { "type": "string" }, "delete": { "type": "boolean" } }),
                &["doc", "pages"],
            )),
        t("doc_combine", "Combine files", "Combine files, in order, into one PDF (bookmarks are kept under one entry per file). Images (PNG, JPEG, TIFF, GIF, BMP, JPEG 2000) and .txt files are made into PDFs first. pages optionally chooses each file's pages, in step with paths: a range such as \"1-3, 6\" or null for all pages. passwords, also in step with paths, opens encrypted files (the open password, or the permissions password where a file's security doesn't allow copying pages; null for none). The result is not encrypted. Passwords are never echoed back. groups, also in step with paths, splits a file around others: entries with the same number (they must name the same path) are one file, copied once, so it keeps one bookmark at its first page, its links and its form fields; null for a file of its own. A group uses its first entry's password. When two files' forms name the same field, field_conflicts says what happens: \"rename\" (the default) keeps the fields separate by renaming the later one (Text1 becomes Text1-1) and the result lists renamed_fields as [[old, new], …] — scripts that refer to the old name are not rewritten; \"merge\" makes them one field, as before.")
            .cmd("page.combine")
            .with(schema(
                json!({
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 2 },
                    "pages": { "type": "array", "items": { "type": ["string", "null"] } },
                    "passwords": { "type": "array", "items": { "type": ["string", "null"] } },
                    "groups": { "type": "array", "items": { "type": ["integer", "null"], "minimum": 0 } },
                    "field_conflicts": { "type": "string", "enum": ["rename", "merge"] },
                    "out": save_out,
                    "open": open,
                }),
                &["paths"],
            )),
        t("doc_create_multiple", "Create PDF from multiple files", "Convert several files (PDFs, PNG/JPEG/JPEG 2000/TIFF/GIF/BMP images, .txt files) to PDF in one run. mode \"combine\" (default) joins them, in order, into one PDF with a bookmark per file; pages optionally chooses each file's pages, in step with paths (a range such as \"1-3, 6\" or null). mode \"separate\" writes one PDF per file into out_dir (files that are already PDFs are skipped; existing files are never overwritten) and reports each file's result.")
            .cmd("create.multiple")
            .with(schema(
                json!({
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "mode": { "type": "string", "enum": ["combine", "separate"] },
                    "pages": { "type": "array", "items": { "type": ["string", "null"] } },
                    "out": save_out.clone(),
                    "open": open.clone(),
                    "out_dir": { "type": "string", "description": "Folder for mode \"separate\"." },
                }),
                &["paths"],
            )),
        t("doc_split", "Split a document", "Split into several files written to out_dir: every N pages, before given pages, at top-level bookmarks (bookmarks: true; files named after them, repeated titles get -2, -3), or by file size (max_mb). Files are <name>-partK.pdf.")
            .cmd("page.split")
            .with(schema(
                json!({ "doc": doc(), "every": { "type": "integer", "minimum": 1 }, "before": pages("that start a new part"), "bookmarks": { "type": "boolean" }, "max_mb": { "type": "number", "exclusiveMinimum": 0 }, "out_dir": { "type": "string" } }),
                &["doc", "out_dir"],
            )),
        t(
            "bookmark_list",
            "List bookmarks",
            "The bookmarks in outline order, one page at a time: each with its path (1-based, as the other bookmark tools take it), title, target page and open state, and its children on the same page. next is the offset of the following page, or null at the end. Bookmarks nested deeper than 33 levels are not listed.",
        )
        .ro()
        .with(schema(
            json!({
                "doc": doc(),
                "offset": { "type": "integer", "minimum": 0, "description": "Where the page starts, counted from 0 (the previous page's next)" },
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Bookmarks in the page (default 100)" }
            }),
            &["doc"],
        )),
        t("bookmark_add", "Add a bookmark", "Add a bookmark that goes to a page, under a parent bookmark (or at the top level), at a position. Undoable.").with(schema(
            json!({ "doc": doc(), "title": { "type": "string", "minLength": 1 }, "page": { "type": "integer", "minimum": 1 }, "parent": path("Parent bookmark (omit for the top level)"), "position": { "type": "integer", "minimum": 1, "description": "1-based position among the parent's children (default: last)." } }),
            &["doc", "title", "page"],
        )),
        t("bookmark_rename", "Rename a bookmark", "Change a bookmark's title. Undoable.")
            .with(schema(json!({ "doc": doc(), "path": path("The bookmark"), "title": { "type": "string", "minLength": 1 } }), &["doc", "path", "title"])),
        t("bookmark_delete", "Delete a bookmark", "Delete a bookmark and the bookmarks under it. Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "path": path("The bookmark") }), &["doc", "path"])),
        t("bookmark_move", "Move a bookmark", "Move a bookmark (with its children) under another parent, or within its level. Undoable.").with(schema(
            json!({ "doc": doc(), "path": path("The bookmark to move"), "parent": path("New parent (omit for the top level)"), "position": { "type": "integer", "minimum": 1, "description": "1-based position among the new parent's children, counted after the bookmark is removed (default: last)." } }),
            &["doc", "path"],
        )),
        t("bookmark_set_page", "Set a bookmark's page", "Point a bookmark at another page. Undoable.")
            .with(schema(json!({ "doc": doc(), "path": path("The bookmark"), "page": { "type": "integer", "minimum": 1 } }), &["doc", "path", "page"])),
        t(
            "bookmark_from_structure",
            "New bookmarks from structure",
            "Make bookmarks from the document's tagged headings (H, H1-H6), nested by level under a new first bookmark titled \"Untitled\". Fails if the document has no tagged headings. Returns the first page of the bookmark tree (see bookmark_list). Undoable.",
        )
        .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("page_number", "Number pages", "Label a range of pages (e.g. i, ii, iii for front matter, or A-1, A-2 for an appendix). Later pages keep their labels. Undoable.").with(schema(
            json!({
                "doc": doc(),
                "from": { "type": "integer", "minimum": 1 },
                "to": { "type": "integer", "minimum": 1 },
                "style": { "type": "string", "enum": ["decimal", "upper-roman", "lower-roman", "upper-alpha", "lower-alpha", "none"], "description": "Numbering style (default decimal; none = prefix only)." },
                "prefix": { "type": "string" },
                "start": { "type": "integer", "minimum": 1, "description": "Number of the first page in the range (default 1)." },
            }),
            &["doc", "from", "to"],
        )),
        t("form_fields", "List form fields", "Every interactive form field: name, type (text, checkbox, radio, combo, list, button, signature), value, options, page, rect (top-left-origin points) and rotation (0, 90, 180 or 270 degrees counterclockwise), read-only and required flags. Push buttons include caption (first widget's normal label) and captions [{widget, caption}] for all widgets (1-based).")
            .ro()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t(
            "form_fill",
            "Fill in form fields",
            "Set several fields at once (one undo step). values maps field names (from form_fields) to: a string for text fields, combo boxes and radio groups (an option), true/false for check boxes, an array of strings for multi-select lists. Appearances are regenerated so every viewer shows the values. A value whose characters the field's font can't draw (Chinese in a Helvetica field) is still saved; `warnings` in the result names the field and character: PdfCraft draws `?` there and asks other viewers to redraw the field from its value.",
        )
        .with(schema(json!({ "doc": doc(), "values": { "type": "object", "description": "Field name → value." } }), &["doc", "values"])),
        t("form_reset", "Clear form", "Reset fields to their default values: all of them, or only those listed. Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "fields": { "type": "array", "items": { "type": "string" } } }), &["doc"])),
        t(
            "form_add_field",
            "Prepare form: add a field",
            "Add a form field on a page. type: text, date, checkbox, radio, combo, list, button, image (a picture placeholder; fill it with form_set_image), signature. rect in points from the top-left of the displayed page [x0, y0, x1, y1]. name defaults to Acrobat's next free name (Text1, Check Box1, Group1, Dropdown1, List Box1, Button1, Image1, Signature1, Date1). Radio buttons join the radio group named by group (a new group otherwise) with the export value export. Returns the field's name. Undoable.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "page": { "type": "integer", "minimum": 1 },
                "type": { "type": "string", "enum": ["text", "date", "checkbox", "radio", "combo", "list", "button", "image", "signature"] },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                "name": { "type": "string" },
                "multiline": { "type": "boolean" },
                "group": { "type": "string", "description": "Radio group to join." },
                "export": { "type": "string", "description": "Radio button export value (default Choice1)." },
                "options": { "type": "array", "items": { "type": "string" }, "description": "Combo box / list box items." },
                "editable": { "type": "boolean", "description": "Combo box accepts typed text." },
                "multi_select": { "type": "boolean" },
                "caption": { "type": "string", "description": "Button label." },
            }),
            &["doc", "page", "type", "rect"],
        )),
        t("stamp_custom", "Add a custom stamp", "Stamp a picture: a PDF page (file_page, default 1) or an image file, centred at at [x, y] (points from the top-left of the displayed page) at its natural size, at most 200 pt. name labels it (default: the file name). Undoable.")
            .cmd("comment.stamp")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "path": { "type": "string" },
                    "at": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                    "file_page": { "type": "integer", "minimum": 1 },
                    "name": { "type": "string" },
                    "author": { "type": "string" },
                }),
                &["doc", "page", "path", "at"],
            )),
        t("form_set_image", "Set an image field's picture", "Show an image file (PNG, JPEG, TIFF, GIF or BMP) in an image field or button, scaled to fit and centred (what clicking an image field does). On an XFA form the picture is kept in its data too (base64, as Designer saves it) and the form shows it from there. Undoable.")
            .with(schema(json!({ "doc": doc(), "field": { "type": "string" }, "path": { "type": "string" } }), &["doc", "field", "path"])),
        t(
            "form_set_props",
            "Field properties",
            "Change field properties: give field for one field, or fields for a list of unique field names (1–1000). A bulk change is atomic and one Undo step; an invalid or locked field leaves every field unchanged. Only the given properties change, and partial appearance changes preserve each field's other colours and font. name (rename), rect (position), rotation and caption require a single field. caption changes one push-button normal label (widget: 1-based index, default 1); an empty string clears it. Other widgets and icon-only layouts are preserved. General and Options: tooltip, read_only, required, multiline, max_length (0 = no limit), options (combo/list items), font_size (0 = auto), format, validate, calculate (Acrobat's native AF rules). Options: align (left, center, right), default (Reset form's value; check boxes/radio buttons use their on state, \"\" for off), flags { scroll, rich_text, password, file_select, spell_check, comb (needs max_length), sort, editable, multi_select, commit_immediately }, check_style (check boxes/radio buttons: check, circle, cross, diamond, square, star). rotation is 0, 90, 180 or 270 degrees counterclockwise and swaps width/height when the axis changes. Undoable.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "field": { "type": "string", "description": "The field's current name." },
                "fields": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 1000, "uniqueItems": true, "description": "Apply the same changes to these fields in one atomic edit. Use either field or fields." },
                "name": { "type": "string" },
                "tooltip": { "type": "string" },
                "caption": { "type": "string", "description": "Normal push-button label; an empty string clears it. Requires a single field." },
                "widget": { "type": "integer", "minimum": 1, "description": "Widget whose caption changes (1-based, default 1). Requires caption. See form_fields.captions." },
                "read_only": { "type": "boolean" },
                "required": { "type": "boolean" },
                "multiline": { "type": "boolean" },
                "max_length": { "type": "integer", "minimum": 0 },
                "options": { "type": "array", "items": { "type": "string" } },
                "align": { "type": "string", "enum": ["left", "center", "right"] },
                "locked": { "type": "boolean", "description": "Lock the field's properties (only unlocking is accepted while locked)." },
                "default": { "type": "string" },
                "check_style": { "type": "string", "enum": ["check", "circle", "cross", "diamond", "square", "star"], "description": "Check boxes and radio buttons: the mark shown when on." },
                "flags": { "type": "object", "additionalProperties": { "type": "boolean" } },
                "font_size": { "type": "number", "minimum": 0 },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "Move/resize: points from the top-left of the displayed page." },
                "rotation": { "type": "integer", "enum": [0, 90, 180, 270], "description": "Widget rotation in degrees, counterclockwise. Changing between horizontal and vertical swaps the rectangle around its center." },
                "appearance": { "type": "object", "description": "Appearance tab: border, fill, text_color (#RRGGBB, a name or \"none\"), width (0–12 points; 1 thin, 2 medium, 3 thick), style (solid, dashed, beveled, inset, underline), font (helvetica, times, courier)." },
                "format": { "description": "Format tab: {\"type\": \"number\", \"decimals\": 2, \"currency\": \"$\", \"separator\": 0-4, \"negative\": 0-3}, {\"type\": \"percent\"}, {\"type\": \"date\"|\"time\", \"pattern\": \"mm/dd/yyyy\"}, {\"type\": \"zip\"|\"zip4\"|\"phone\"|\"ssn\"}, {\"type\": \"mask\", \"mask\": \"AA-9999\"}, or \"none\"." },
                "validate": { "description": "Validate tab: {\"min\": 0, \"max\": 100} (either may be left out) or \"none\"." },
                "calculate": { "description": "Calculate tab: {\"op\": \"sum\"|\"product\"|\"average\"|\"min\"|\"max\", \"fields\": [\"a\", \"b\"]}, {\"notation\": \"Price * Qty\"} (simplified field notation), or \"none\"." },
            }),
            &["doc"],
        )),
        t("form_tab_order", "Set the tab order", "Set the tab order of pages (default all): row (top to bottom, left to right), column, structure, or annotations (unspecified). Or order tabs manually: `field` with `move` earlier|later moves that field one place on its page. Returns the resulting order of fields. Undoable.")
            .with(schema(
                json!({ "doc": doc(), "order": { "type": "string", "enum": ["row", "column", "structure", "annotations"] }, "pages": pages("to set (default: all)"), "field": { "type": "string" }, "move": { "type": "string", "enum": ["earlier", "later"] } }),
                &["doc"],
            )),
        t("doc_export_data", "Export comments or form data", "Write comments and/or form data to path; the extension picks the format: .xfdf or .fdf (comments and/or fields), .xml, .csv or .txt (form data). For an XFA form, .xml is its data alone (rooted at the form's root data node, as Acrobat exports it) and .xdp an XML Data Package holding the whole datasets. what: all (default), comments, fields.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" }, "what": { "type": "string", "enum": ["all", "comments", "fields"] } }), &["doc", "path"])),
        t("doc_import_data", "Import comments or form data", "Import comments and/or field values from an XFDF, FDF, XML, CSV or tab-delimited text file (detected from its content). Comments with the same name are replaced; values go through the form's formats and validation. For an XFA form, an XDP, datasets or data XML file replaces the form's data: a laid-out form is laid out again from it (rows included) and its calculations run. Undoable.")
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" } }), &["doc", "path"])),
        t("form_delete_field", "Delete a field", "Delete a form field and all its widgets. Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "field": { "type": "string" } }), &["doc", "field"])),
        t(
            "redact_mark",
            "Mark for redaction",
            "Mark content for redaction (nothing is removed until redact_apply). One of: rect [x0, y0, x1, y1] (points from the top-left of the displayed page) with page; find (text, every match); words (a list of words or phrases, every match of each; the result's matched_words says how many marks each made); pattern (phone, email, credit-card, ssn, date: every match, Acrobat's Search & Redact patterns); whole_pages: true. find and words accept case_sensitive and whole_words (both default false). find, words, pattern and whole_pages work on pages (default all). overlay: text shown on the box once applied, or code_set (foia: U.S. FOIA (b)(1)(A)…(b)(9); privacy-act: U.S. Privacy Act (d)(5), (j)(1)…(k)(7)) with codes, shown as \"(b)(6), (b)(7)(C)\"; fill: box colour (default black). Undoable.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "page": { "type": "integer", "minimum": 1 },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                "find": { "type": "string", "minLength": 1 },
                "words": { "type": "array", "items": { "type": "string", "minLength": 1 }, "minItems": 1, "maxItems": 1000 },
                "case_sensitive": { "type": "boolean", "description": "Match letter case for find or words (default false)." },
                "whole_words": { "type": "boolean", "description": "Match whole words for find or words (default false)." },
                "pattern": { "type": "string", "enum": ["phone", "email", "credit-card", "ssn", "date"] },
                "whole_pages": { "type": "boolean" },
                "pages": pages("to search or mark (default: all)"),
                "overlay": { "type": "string" },
                "code_set": { "type": "string", "enum": ["foia", "privacy-act"] },
                "codes": { "type": "array", "items": { "type": "string" }, "minItems": 1, "description": "Redaction codes from code_set, e.g. [\"(b)(6)\"]." },
                "fill": { "type": "string", "description": "#RRGGBB or a colour name." },
                "author": { "type": "string" },
            }),
            &["doc"],
        )),
        t(
            "redact_apply",
            "Apply redactions",
            "Apply the redaction marks (all, or those on pages): text, images, vectors, comments and form fields under them are removed for good and boxes are drawn in their place. A verification pass fails the operation if anything readable remains. Undoable until saved; the saved file no longer contains the content.",
        )
        .destructive()
        .with(schema(json!({ "doc": doc(), "pages": pages("whose marks to apply (default: all)") }), &["doc"])),
        t("redact_clear", "Remove redaction marks", "Remove every redaction mark without applying it. Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t(
            "page_cut",
            "Cut pages (Knife)",
            "Cut pages into parts, each part a page of its own right after the original: axis vertical (side by side, left first) or horizontal (one above the other, top first), at the given places measured on the page as shown (units pt from the left or top edge, the default, or fraction of its width or height), the same on every page in pages. right_to_left: true puts the right (or bottom) part first, for books read that way. Each part shows its part through its media and crop boxes; nothing is erased unless trim_outside: true, which erases each part's text, images and drawings outside it (as page_erase does), so a half's text is its own. Annotations go to the part holding their centre. Returns cut: each page's parts, 1-based. Undoable.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "pages": pages("to cut"),
                "axis": { "type": "string", "enum": ["vertical", "horizontal"] },
                "at": { "type": "array", "items": { "type": "number" }, "minItems": 1, "maxItems": 15 },
                "units": { "type": "string", "enum": ["pt", "fraction"] },
                "right_to_left": { "type": "boolean" },
                "trim_outside": { "type": "boolean" },
            }),
            &["doc", "pages", "axis", "at"],
        )),
        t(
            "page_erase",
            "Erase content",
            "Erase the page's content under rects ([x0, y0, x1, y1] each, points from the top-left of the displayed page): glyphs are cut out of their lines with the rest in place, images and vectors removed or clipped. kinds: any of text, images, vectors (the default, all three) and annotations (comments, links and fields there; off by default). rule: centre (default: a glyph goes when its centre is inside, as a selection takes it) or overlap (any real overlap, as redaction). No box is drawn and nothing is marked. An ordinary edit, undoable and saved incrementally, so an earlier revision still holds the content: use redact_mark and redact_apply to remove it for good. Returns erased: how many glyphs, images and path parts went.",
        )
        .destructive()
        .with(schema(
            json!({
                "doc": doc(),
                "page": { "type": "integer", "minimum": 1 },
                "rects": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "minItems": 1, "maxItems": 4096 },
                "kinds": { "type": "array", "items": { "type": "string", "enum": ["text", "images", "vectors", "annotations"] }, "minItems": 1 },
                "rule": { "type": "string", "enum": ["centre", "overlap"] },
            }),
            &["doc", "page", "rects"],
        )),
        t(
            "doc_hidden_info",
            "Find hidden information",
            "What Remove Hidden Information would remove, by category (metadata, attachments, comments, form-fields, hidden-text, hidden-layers, bookmarks, links-actions-scripts, private-data) with counts.",
        )
        .ro()
        .with(schema(json!({ "doc": doc() }), &["doc"])),
        t(
            "doc_remove_hidden",
            "Remove hidden information / Sanitize",
            "Remove the listed categories (see doc_hidden_info), or every category when none are listed (Sanitize Document). Form fields are flattened so their values stay visible. The next save rewrites the whole file. Undoable until saved.",
        )
        .destructive()
        .with(schema(
            json!({
                "doc": doc(),
                "categories": { "type": "array", "items": { "type": "string", "enum": ["metadata", "attachments", "comments", "form-fields", "hidden-text", "hidden-layers", "bookmarks", "links-actions-scripts", "private-data"] } },
            }),
            &["doc"],
        )),
        t(
            "doc_audit_redactions",
            "Audit faux redactions",
            "Find faux redactions: opaque filled rectangles painted over text that is still extractable, and /Redact annotations that were marked but never applied. Paint-order and colour-aware: a rectangle counts only when painted after the text and dark enough to hide it (page backgrounds and light shading are not flagged). Each finding reports the 1-based page, the kind (covered-text or unapplied-mark), the rectangle (points from the top-left of the displayed page, like comment_list), and the hidden text. Fails closed: unreadable content is an error, never a clean bill. A properly applied redaction removes the text and never flags.",
        )
        .ro()
        .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("printers", "List printers", "The printers the system's print spooler knows (CUPS on macOS and Linux), with the default marked.").ro().with(schema(json!({}), &[])),
        t("printer_options", "List printer options", "A printer driver's own job options (CUPS: from its PPD), such as the paper tray, paper type or finishing: key, label, group, default and choices. Pass chosen values to doc_print as options. Empty on Windows, where the driver's preferences window holds them.")
            .ro()
            .with(schema(json!({ "printer": { "type": "string" } }), &["printer"])),
        t(
            "doc_print",
            "Print",
            "Print with Acrobat's Print dialog options, or save the print-ready PDF. pages: a range such as \"1-3, 6, 9-\" (page labels allowed; default all); subset odd/even; reverse. layout: fit (default), actual, shrink, custom (scale %), multiple (per_sheet 2/4/6/9/16, order, border, auto_rotate; cut-stack order arranges single-sided sheets for cutting into piles and stacking left to right, top to bottom, keeping sheet order within each pile; duplex must be off), booklet (booklet_subset both/front/back, binding left/right), poster (scale %, overlap pt, cut_marks). orientation auto/portrait/landscape; comments_forms document / document-and-markups (default) / document-and-stamps / form-fields-only; paper Letter/Legal/Tabloid, ARCH A/B/C/D/E1/E, ANSI C/D/E, or A0/A1/A2/A3/A4/A5/A6 (short names or full preset labels); use layout actual for 100% in the print-ready PDF; printer drivers may still substitute paper or scale. Then path (save) or printer (a name or \"default\") with copies, collate, duplex off/long-edge/short-edge, grayscale, and options (driver options from printer_options, CUPS).",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "pages": { "type": "string" },
                "subset": { "type": "string", "enum": ["all", "odd", "even"] },
                "reverse": { "type": "boolean" },
                "layout": { "type": "string", "enum": ["fit", "actual", "shrink", "custom", "multiple", "booklet", "poster"] },
                "scale": { "type": "number", "exclusiveMinimum": 0 },
                "per_sheet": { "type": "integer", "minimum": 1, "maximum": 256 },
                "order": { "type": "string", "enum": ["horizontal", "horizontal-reversed", "vertical", "vertical-reversed", "cut-stack"] },
                "border": { "type": "boolean" },
                "auto_rotate": { "type": "boolean" },
                "booklet_subset": { "type": "string", "enum": ["both", "front", "back"] },
                "binding": { "type": "string", "enum": ["left", "right"] },
                "overlap": { "type": "number", "minimum": 0 },
                "cut_marks": { "type": "boolean" },
                "orientation": { "type": "string", "enum": ["auto", "portrait", "landscape"] },
                "comments_forms": { "type": "string", "enum": ["document", "document-and-markups", "document-and-stamps", "form-fields-only"] },
                "paper": { "type": "string" },
                "path": { "type": "string" },
                "printer": { "type": "string" },
                "copies": { "type": "integer", "minimum": 1, "maximum": 999 },
                "collate": { "type": "boolean" },
                "duplex": { "type": "string", "enum": ["off", "long-edge", "short-edge"] },
                "grayscale": { "type": "boolean" },
                "as_image": { "type": "boolean", "description": "Print as image (Acrobat's Advanced Print Setup): every page rendered to a picture at dpi before it is placed on the paper, for printers that skip text, shading or transparency; the print is no longer searchable text, and with grayscale the pictures are grey." },
                "dpi": { "type": "number", "minimum": 72, "maximum": 600, "description": "Resolution for as_image (default 300)." },
                "options": { "type": "object", "additionalProperties": { "type": "string" }, "description": "Printer driver options from printer_options: key → choice value." },
            }),
            &["doc"],
        )),
        t("content_list", "List added content", "Text and images added with page_add_text/page_add_image (or Edit a PDF ▸ Add content), per page with a 1-based index, rect (points from the top-left of the page), and text style.")
            .ro()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t(
            "page_add_text",
            "Add text to a page",
            "Add text as page content (not a comment). Place it with at [x, y] (top-left, points from the top-left of the displayed page) and width (wrap width, default 200), or rect. Newlines start new lines; long lines wrap. Style: font helvetica/times/courier, size, bold, italic, color (#RRGGBB or a name), align left/center/right. Text must be Western European (WinAnsi) characters, or Arabic, Hebrew or Thai when the build has their craft-fonts faces (drawn right to left where the text is); others are refused. It stays editable with content_update. Undoable.",
        )
        .cmd("edit.text")
        .with(schema(
            json!({
                "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "text": { "type": "string", "minLength": 1 },
                "at": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 },
                "width": { "type": "number", "exclusiveMinimum": 0 },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                "font": { "type": "string", "enum": ["helvetica", "times", "courier"] }, "size": { "type": "number", "minimum": 1, "maximum": 500 },
                "bold": { "type": "boolean" }, "italic": { "type": "boolean" }, "color": { "type": "string" },
                "align": { "type": "string", "enum": ["left", "center", "right", "justify"] },
            }),
            &["doc", "page", "text"],
        )),
        t(
            "page_add_image",
            "Add an image to a page",
            "Add an image file (PNG, JPEG, TIFF, GIF, BMP) as page content: in rect (points from the top-left of the page), or centred at its natural size (shrunk to fit). Undoable; movable with content_update.",
        )
        .cmd("edit.image")
        .with(schema(
            json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "path": { "type": "string" }, "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 } }),
            &["doc", "page", "path"],
        )),
        t("content_update", "Edit added content", "Move/resize (rect), retype (text) or restyle (font, size, bold, italic, color, align) an added item (page, index from content_list). Images: rotate (degrees, multiple of 90, counter-clockwise), flip_h / flip_v (toggle), crop [left, bottom, right, top] as fractions trimmed, image (a file that replaces the picture). Undoable.")
            .with(schema(
                json!({
                    "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "index": { "type": "integer", "minimum": 1 },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 }, "text": { "type": "string" },
                    "font": { "type": "string", "enum": ["helvetica", "times", "courier"] }, "size": { "type": "number", "minimum": 1, "maximum": 500 },
                    "bold": { "type": "boolean" }, "italic": { "type": "boolean" }, "color": { "type": "string" },
                    "align": { "type": "string", "enum": ["left", "center", "right", "justify"] },
                    "rotate": { "type": "integer" }, "flip_h": { "type": "boolean" }, "flip_v": { "type": "boolean" },
                    "crop": { "type": "array", "items": { "type": "number", "minimum": 0, "maximum": 0.49 }, "minItems": 4, "maxItems": 4 },
                    "image": { "type": "string", "description": "Replace the picture with this file." },
                }),
                &["doc", "page", "index"],
            )),
        t("content_delete", "Delete added content", "Delete an added item (page, index from content_list). Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "index": { "type": "integer", "minimum": 1 } }), &["doc", "page", "index"])),
        t("link_list", "List links", "Every link: page, 1-based index (for link_edit/link_delete), rect (points from the top-left of the page), and where it goes (url or to_page).")
            .ro()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("link_open", "Open a linked PDF", "Follow a local GoToR link in PdfCraft: open the referenced PDF as a new document, respecting --root. page and index use link_list numbering. Named destinations are reported but not applied. Absolute file specifications require allow_absolute: true after reviewing the target; relative links need no opt-in.")
            .with(schema(
                json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "index": { "type": "integer", "minimum": 1 }, "password": { "type": "string", "description": "Optional password for the linked PDF." }, "allow_absolute": { "type": "boolean", "description": "Explicitly allow an absolute local GoToR target (default false). Existing validation and --root confinement still apply." } }),
                &["doc", "page", "index"],
            )),
        t("link_add", "Add a link", "Add a link over rect on page that opens url or goes to to_page. Appearance: visible (rectangle), color, width 1-3, highlight none/invert/outline/inset. Undoable.")
            .cmd("edit.link")
            .with(schema(
                json!({
                    "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                    "url": { "type": "string" }, "to_page": { "type": "integer", "minimum": 1 },
                    "visible": { "type": "boolean" }, "color": { "type": "string" }, "width": { "type": "number" }, "highlight": { "type": "string", "enum": ["none", "invert", "outline", "inset"] },
                }),
                &["doc", "page", "rect"],
            )),
        t("link_edit", "Edit a link", "Change a link's rect, destination (url or to_page) or appearance. Undoable.")
            .with(schema(
                json!({
                    "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "index": { "type": "integer", "minimum": 1 },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                    "url": { "type": "string" }, "to_page": { "type": "integer", "minimum": 1 },
                    "visible": { "type": "boolean" }, "color": { "type": "string" }, "width": { "type": "number" }, "highlight": { "type": "string", "enum": ["none", "invert", "outline", "inset"] },
                }),
                &["doc", "page", "index"],
            )),
        t("link_delete", "Delete a link", "Delete one link (page, index from link_list). Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "index": { "type": "integer", "minimum": 1 } }), &["doc", "page", "index"])),
        t("links_from_urls", "Create links from URLs", "Find web addresses (http://, https://, www.) in the text of every page and make them clickable links. Returns the URLs. Undoable.")
            .cmd("edit.links_from_urls")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("links_remove", "Remove all links", "Remove every link in the document (or on the given pages). Undoable.")
            .destructive()
            .cmd("edit.remove_links")
            .with(schema(json!({ "doc": doc(), "pages": pages("whose links to remove (default: all)") }), &["doc"])),
        t("measure_distance", "Measure distance", "Add an undoable two-point distance annotation using the scale of the first point's viewport.")
            .cmd("measure.distance").with(measure_schema()),
        t("measure_perimeter", "Measure perimeter", "Add an undoable connected-line length annotation. To include a closing edge, repeat the first point at the end.")
            .cmd("measure.perimeter").with(measure_schema()),
        t("measure_area", "Measure area", "Add an undoable area annotation from a simple polygon. The last edge closes automatically.")
            .cmd("measure.area").with(measure_schema()),
        t("measure_info", "Read a measurement", "Calculate a live distance, perimeter or area, deltas, angle and scale without adding an annotation. Incomplete paths are allowed.")
            .ro().cmd("measure.info").with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},"points":measure_points(),"type":{"type":"string","enum":["distance","perimeter","area"]}}), &["doc","page","points"])),
        t("measure_list", "List measurements", "Saved measurement annotations with calculated values, scale and vertices in display coordinates. Measurements with unsupported imported formats (compound or fractional units, non-rectilinear scales) or invalid geometry are listed under unsupported with a reason.")
            .ro().with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1}}), &["doc"])),
        t("measure_scale", "Set or read a measurement scale", "Read the scale at a point, or add a rectangular viewport using units_per_point or two calibration points and their real-world distance. Existing measurements retain their original scales. Undoable.")
            .cmd("measure.scale").with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},"at":point(),"rect":{"type":"array","items":{"type":"number"},"minItems":4,"maxItems":4},"name":{"type":"string"},"unit":{"type":"string"},"precision":{"type":"integer","minimum":0,"maximum":6},"units_per_point":{"type":"number","exclusiveMinimum":0},"points":measure_points(),"distance":{"type":"number","exclusiveMinimum":0}}), &["doc","page"])),
        t("measure_snap", "Snap a measurement vertex", "Snap a point to vector paths, endpoints, midpoints or intersections. Coordinates and tolerance are in display points. Bounded extraction reports truncated geometry.")
            .ro().cmd("measure.snap").with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},"at":point(),"tolerance":{"type":"number","minimum":0,"maximum":10000},"endpoints":{"type":"boolean"},"midpoints":{"type":"boolean"},"intersections":{"type":"boolean"},"paths":{"type":"boolean"}}), &["doc","page","at"])),
        t("measure_export", "Export measurements as CSV", "Atomically write saved measurement values, labels, authors and scale ratios as spreadsheet-safe CSV. Returns how many unsupported measurements were left out.")
            .cmd("measure.export").with(schema(json!({"doc":doc(),"out":path_arg()}), &["doc","out"])),
        t("comment_list", "List comments", "Every comment (annotation other than links, form widgets and pop-ups) with its page, index, id, type, author, text, date, rectangle, colour, review status and replies.")
            .ro()
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1, "description": "Only this page." } }), &["doc"])),
        t(
            "comment_add",
            "Add a comment",
            "Add a comment as Acrobat's commenting tools do. Geometry is in points with the origin at the top-left of the displayed page, y down (as in page_render images at 72 dpi and text_find rects). \
             note: `at` [x, y] (icon top-left). stamp: `at` [x, y] (its centre) and `stamp`: approved, completed, confidential, draft, final, for comment, for public release, information only, not approved, not for public release, preliminary results, void, accepted, initial here, rejected, sign here, witness; dynamic: true for the dynamic approved/confidential/received/reviewed/revised stamps with a By … at … line. highlight/underline/strikeout/squiggly: `find` (text on the page to mark; every match with all: true) or `quads`. \
             rectangle/oval/textbox: `rect` [x0, y0, x1, y1]. line/arrow: `from`, `to`. Optional `endings` (two names: start, end) for a line, arrow or polyline, or one name for a callout: None, Square, Circle, Diamond, OpenArrow, ClosedArrow, Butt, ROpenArrow, RClosedArrow, Slash. An arrow without endings is None then OpenArrow. ink: `strokes` [[[x, y], …], …]. \
             polygon/cloud/polyline (connected lines): `points` [[x, y], …]. callout: `rect` (its text box), `to` (the point the arrow touches), optional `knee`. caret (inserted text): `at`, the insertion point on the baseline. replace (Replace Text): `find` or `quads` like highlight, `contents` the replacement. attachment: `path` (the file), `at`, optional `icon` (PushPin, Paperclip, Graph, Tag). Undoable. Text a text box, callout or stamp draws in the standard fonts that they can't draw (Chinese, Greek) is saved as typed and drawn as `?`; `warnings` in the result says so.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "page": { "type": "integer", "minimum": 1 },
                "type": { "type": "string", "enum": ["note", "highlight", "underline", "strikeout", "squiggly", "replace", "rectangle", "oval", "line", "arrow", "ink", "textbox", "stamp", "polygon", "cloud", "polyline", "callout", "caret", "attachment"] },
                "path": { "type": "string", "description": "attachment: the file to attach." },
                "points": { "type": "array", "items": point(), "minItems": 2 },
                "knee": point(),
                "contents": { "type": "string", "description": "The comment text (what a text box shows)." },
                "author": { "type": "string" },
                "at": point(),
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                "from": point(),
                "to": point(),
                "find": { "type": "string", "description": "Text on the page to mark up (case-insensitive)." },
                "all": { "type": "boolean", "description": "Mark every match of `find` on the page, not just the first." },
                "quads": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 8, "maxItems": 8 } },
                "stamp": { "type": "string" },
                "dynamic": { "type": "boolean" },
                "strokes": { "type": "array", "items": { "type": "array", "items": point() } },
                "icon": { "type": "string", "description": "note: Comment, Note, Help, Insert, Key, NewParagraph, Paragraph; attachment: PushPin, Paperclip, Graph, Tag." },
                "color": color(),
                "fill": color(),
                "opacity": { "type": "number", "minimum": 0, "maximum": 1 },
                "width": { "type": "number", "minimum": 0, "description": "Line width in points." },
                "endings": { "type": "array", "items": { "type": "string", "enum": ["None", "Square", "Circle", "Diamond", "OpenArrow", "ClosedArrow", "Butt", "ROpenArrow", "RClosedArrow", "Slash"] }, "minItems": 1, "maxItems": 2, "description": "Line endings. Two names (start, end) for a line, arrow or polyline; one name for a callout." },
                "font_size": { "type": "number", "exclusiveMinimum": 0 },
            }),
            &["doc", "page", "type"],
        )),
        t("comment_reply", "Reply to a comment", "Add a reply to a comment's thread. Undoable.").with(schema(
            comment_ref(json!({ "text": { "type": "string", "minLength": 1 }, "author": { "type": "string" } })),
            &["doc", "text"],
        )),
        t("comment_set_status", "Set a comment's status", "Set a comment's review status (Acrobat: Set status), recorded as a status reply. Undoable.").with(schema(
            comment_ref(json!({ "status": { "type": "string", "enum": ["none", "accepted", "rejected", "cancelled", "completed"] }, "author": { "type": "string" } })),
            &["doc", "status"],
        )),
        t("sign_list", "List and validate signatures", "Every signature field with its validation: status (valid, unknown = intact but the signer isn't trusted, invalid, unsigned), signer and certificate, date, reason, location, certification level, page and rect, the revision it covers and changes made after signing (none, allowed, disallowed), with Acrobat-style explanations.")
            .ro()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("sign_id_create", "Create a digital ID", "Create a self-signed digital ID and save it as a password-protected .p12 file (Acrobat: Configure a new digital ID ▸ Create a new digital ID ▸ Save to file). Default key: 2048-bit RSA, valid 5 years.").with(schema(
            json!({
                "name": { "type": "string", "minLength": 1 },
                "organization": { "type": "string" },
                "unit": { "type": "string" },
                "email": { "type": "string" },
                "country": { "type": "string", "description": "Two-letter country code." },
                "key": { "type": "string", "enum": ["rsa2048", "rsa3072", "rsa4096", "p256"] },
                "years": { "type": "integer", "minimum": 1, "maximum": 50 },
                "password": { "type": "string", "minLength": pdfcraft_engine::sign::pkcs12::MIN_PASSWORD_CHARS },
                "path": { "type": "string", "description": "The .p12 file to write." },
            }),
            &["name", "password", "path"],
        )),
        t("sign_windows_ids", "List Windows store digital IDs", "Windows: signing identities in the Current User Personal certificate store (certificate details and the windows: reference sign_document takes). Private keys remain in CNG; Windows may ask permission to use them. `unusable` lists the store's other certificates with why they can't sign (no private key, an unsupported key type, a key CNG can't open).")
            .ro()
            .cmd("sign.digital")
            .with(schema(json!({}), &[])),
        t("sign_keychain_ids", "List Keychain digital IDs", "macOS: the signing identities in the user's keychains (certificate details and the keychain: reference sign_document takes). The private keys stay in the Keychain, which may ask the user to allow their use.")
            .ro()
            .cmd("sign.digital")
            .with(schema(json!({}), &[])),
        t("sign_document", "Sign a document", "Sign with a digital ID (a .p12/.pfx path, or on macOS a Keychain identity: \"keychain:<common name or fingerprint>\" from sign_keychain_ids, or on Windows a store identity: \"windows:<common name or fingerprint>\" from sign_windows_ids) and save the signed file to `out` (signing always saves, as in Acrobat; the document then shows the signed file). Sign an existing empty signature field (`field`), or a new one on `page` at `rect` (omit rect for an invisible signature). certify: no_changes, form_fill or comments makes a certification signature. graphic shows a picture where the signer's name would be large (Acrobat: Configure Signature Appearance): {\"image\": path} (PNG or JPEG), {\"text\": name} (typed in the Fill & Sign script font) or {\"strokes\": [[[x, y], …], …]} (y up, the drawing pad 1 wide); it needs a visible signature. appearance chooses what a visible signature shows (default: name, date and labels; reason, location and distinguished_name are off unless turned on). labels gives the words of its labels, e.g. in another language (default English: \"Digitally signed by\", \"DN: \", \"Reason: \", \"Location: \", \"Date: \"; include the separator); each must be in Western European characters (Windows-1252), the only ones the signature's font draws, or the call is refused naming the label and character. PAdES B-B, SHA-256 (SHA-384 for P-384 keys).")
            .cmd("sign.digital")
            .with(schema(
                json!({
                    "doc": doc(),
                    "id": { "type": "string", "description": "Digital ID file (.p12 / .pfx), keychain: reference on macOS, or windows: reference on Windows." },
                    "password": { "type": "string" },
                    "field": { "type": "string" },
                    "page": { "type": "integer", "minimum": 1 },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                    "reason": { "type": "string" },
                    "location": { "type": "string" },
                    "contact": { "type": "string" },
                    "certify": { "type": "string", "enum": ["no_changes", "form_fill", "comments"] },
                    "graphic": {
                        "type": "object",
                        "description": "Exactly one of image, text or strokes.",
                        "properties": {
                            "image": { "type": "string", "description": "A PNG or JPEG file." },
                            "text": { "type": "string", "description": "Typed in the script font." },
                            "strokes": { "type": "array", "items": { "type": "array", "items": { "type": "array", "items": { "type": "number" }, "minItems": 2, "maxItems": 2 } } },
                        },
                        "minProperties": 1,
                        "maxProperties": 1,
                    },
                    "appearance": {
                        "type": "object",
                        "description": "What a visible signature shows; items not given keep their default.",
                        "properties": {
                            "name": { "type": "boolean", "description": "Default true." },
                            "date": { "type": "boolean", "description": "Default true." },
                            "reason": { "type": "boolean", "description": "Default false." },
                            "location": { "type": "boolean", "description": "Default false." },
                            "distinguished_name": { "type": "boolean", "description": "Default false." },
                            "labels": { "type": "boolean", "description": "Default true." },
                        },
                        "additionalProperties": false,
                    },
                    "labels": {
                        "type": "object",
                        "description": "The words of a visible signature's labels (at most 200 characters each, Western European characters only); labels not given stay English.",
                        "properties": {
                            "signed_by": { "type": "string", "description": "Above the name. Default \"Digitally signed by\"." },
                            "distinguished_name": { "type": "string", "description": "Before the certificate's subject. Default \"DN: \"." },
                            "reason": { "type": "string", "description": "Default \"Reason: \"." },
                            "location": { "type": "string", "description": "Default \"Location: \"." },
                            "date": { "type": "string", "description": "Default \"Date: \"." },
                        },
                        "additionalProperties": false,
                    },
                    "out": { "type": "string", "description": "Where to save the signed document." },
                }),
                &["doc", "id", "out"],
            )),
        t("sign_trust", "Trust certificates", "Add certificates (.cer/.crt/.pem/.der, or the certificates in a .p12 with `password`) to the trusted certificates used to validate signatures, or clear the list (clear: true). Every open document is revalidated. Returns the trusted list.").with(schema(
            json!({ "paths": { "type": "array", "items": { "type": "string" } }, "password": { "type": "string" }, "clear": { "type": "boolean" } }),
            &[],
        )),
        t("comment_mark", "Mark a comment with a checkmark", "Mark or unmark a comment with a checkmark (Acrobat: Mark with checkmark), recorded as a private reply. Undoable.").with(schema(
            comment_ref(json!({ "marked": { "type": "boolean", "description": "Default true." }, "author": { "type": "string" } })),
            &["doc"],
        )),
        t("comment_lock", "Lock a comment", "Lock or unlock a comment (Properties ▸ Locked). Locked comments can't be moved, resized, restyled or deleted; their text stays editable. Undoable.").with(schema(
            comment_ref(json!({ "locked": { "type": "boolean", "description": "Default true." } })),
            &["doc"],
        )),
        t("comments_hide", "Hide all comments", "Hide or show every comment on the page (fields and links still draw). A view setting: the file doesn't change.")
            .cmd("comment.hide_all")
            .with(schema(json!({ "doc": doc(), "hidden": { "type": "boolean", "description": "Default true." } }), &["doc"])),
        t("view_reading_colors", "Set reading colours", "Set the document's reading colours (Acrobat's Replace Document Colors, in the Reading Options): solid text and line-art paints are remapped for readability and the page clears to the background colour in page_render. A view setting: the file doesn't change. mode is one of: off, black-on-white, white-on-black, yellow-on-black, green-on-black, or custom (which takes text and background as #RRGGBB). only_black maps only near-black paints (default false) and line_art also maps line-art paints (default true); both override the mode's preset. A flag that differs from the mode's preset default makes the returned mode read custom, because the resulting params no longer match any preset. Returns the resulting setting.")
            .cmd("a11y.reading_options")
            .with(schema(
                json!({
                    "doc": doc(),
                    "mode": { "type": "string", "enum": ["off", "black-on-white", "white-on-black", "yellow-on-black", "green-on-black", "custom"] },
                    "text": { "type": "string", "description": "With mode custom: the text colour, #RRGGBB." },
                    "background": { "type": "string", "description": "With mode custom: the background colour, #RRGGBB." },
                    "only_black": { "type": "boolean", "description": "Map only near-black paints. Default false." },
                    "line_art": { "type": "boolean", "description": "Also map line-art paints. Default true." },
                }),
                &["doc", "mode"],
            )),
        t("comments_summarize", "Summarize comments", "Make a PDF summarising every comment (number, author, type, date, text, replies), sorted by page, author, date or type.")
            .cmd("comment.summarize")
            .with(schema(
                json!({ "doc": doc(), "sort": { "type": "string", "enum": ["page", "author", "date", "type"] }, "out": save_out, "open": open }),
                &["doc"],
            )),
        t("comment_edit", "Edit a comment", "Change a comment's text, colour, opacity, line width, font size (text box/typewriter/callout), fill (rectangle/oval/polygon/text box/callout; \"none\" removes it), rectangle (rectangle/oval/text box/stamp) or position (`move` [dx, dy] in points). Stamps keep their original appearance when resized. One undo step.").with(schema(
            comment_ref(json!({
                "contents": { "type": "string" },
                "color": color(),
                "opacity": { "type": "number", "minimum": 0, "maximum": 1 },
                "width": { "type": "number", "minimum": 0 },
                "fill": { "type": "string", "description": "Interior colour of a rectangle, oval, polygon, text box or callout: #RRGGBB, a colour name, or \"none\" for no fill." },
                "font_size": { "type": "number", "minimum": 1, "maximum": 400, "description": "Font size in points of a text box, typewriter text or callout; the box grows to hold the text." },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                "move": point(),
            })),
            &["doc"],
        )),
        t("comment_image_preview", "Preview image signature layers", "Return a PNG of the page without the selected image signature/initials (layer=background, default), or its embedded image with alpha (layer=image). Includes the displayed rectangle, document rotation, image_rotation (how far clockwise the image is shown turned) and annotation opacity. Cache these layers for live placement/resizing; commit once with comment_edit. Does not change the document or undo history.")
            .ro().with(schema(comment_ref(json!({ "layer": { "type": "string", "enum": ["background", "image"], "default": "background" }, "dpi": { "type": "number", "minimum": 1, "maximum": 600, "default": 96 } })), &["doc"])),
        t("comment_delete", "Delete a comment", "Delete a comment with its pop-up and replies. Undoable.").destructive().with(schema(comment_ref(json!({})), &["doc"])),
        t(
            "doc_protect",
            "Protect with passwords",
            "Encrypt the document (applied by the next doc_save, a full rewrite). open_password is needed to open it. permissions_password is needed to change security and lifts the restrictions given by printing/changes/copy/accessibility; those restrictions (and their defaults) apply only when permissions_password is given. With open_password alone the document is encrypted and everything stays allowed, so passing a restriction without permissions_password is an error. Passwords are never echoed back. Undoable.",
        )
        .cmd("protect.password")
        .with(schema(
            json!({
                "doc": doc(),
                "open_password": { "type": "string", "minLength": 1, "description": "Required to open the document. On its own it restricts nothing." },
                "permissions_password": { "type": "string", "minLength": 1, "description": "Required to change security; enables printing/changes/copy/accessibility and their defaults." },
                "printing": { "type": "string", "enum": ["none", "low", "high"], "description": "Needs permissions_password. Default high." },
                "changes": { "type": "string", "enum": ["none", "pages", "fill-sign", "comment-fill-sign", "any-except-extract"], "description": "Needs permissions_password. Default none." },
                "copy": { "type": "boolean", "description": "Allow copying text and images (needs permissions_password; default false)." },
                "accessibility": { "type": "boolean", "description": "Allow screen readers to read the text (needs permissions_password; default true)." },
                "compatibility": { "type": "string", "enum": ["aes-256", "aes-128", "rc4-128", "rc4-40"], "description": "Default aes-256 (Acrobat X and later)." },
                "encrypt_metadata": { "type": "boolean", "description": "Default true." },
            }),
            &["doc"],
        )),
        t("doc_unprotect", "Remove security", "Remove password security (the document must have been opened with its permissions password, or have none). Applied by the next doc_save. Undoable.")
            .cmd("protect.remove")
            .destructive()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("page_replace", "Replace pages", "Replace the content of pages with pages of another PDF (same count). Links, comments, form fields and bookmarks on the original pages stay, as in Acrobat. Undoable.")
            .cmd("page.replace")
            .with(schema(json!({ "doc": doc(), "pages": pages("to replace"), "path": path_arg(), "from_pages": pages("of the other file, in order (default: its first pages)") }), &["doc", "pages", "path"])),
        t("page_duplicate", "Duplicate pages", "Insert copies of pages so the first copy becomes page `at`, or after the last of them when `at` is left out (fonts and images are shared, not copied). Undoable.")
            .cmd("page.duplicate")
            .with(schema(json!({ "doc": doc(), "pages": pages("to duplicate"), "at": { "type": "integer", "minimum": 1 } }), &["doc", "pages"])),
        t(
            "page_transition_set",
            "Set page transitions",
            "Set or remove the transition and display time pages play in a full-screen presentation (ISO 32000-2 §12.4.4; Acrobat's Organize ▸ Page Transitions). style: Split, Blinds, Box, Wipe, Dissolve, Glitter, R (replace, no effect), Fly, Push, Cover, Uncover or Fade, or none to remove the transition; duration in seconds (default 1); dimension horizontal/vertical (Split, Blinds); motion inward/outward (Split, Box, Fly); direction 0, 90, 180, 270, 315 or none (Wipe, Glitter, Fly, Cover, Uncover, Push); scale and opaque for Fly. display: seconds the page stays before advancing; left out, the page waits for the presenter. doc_info lists the result under transitions.",
        )
        .with(schema(
            json!({
                "doc": doc(),
                "pages": pages("to change (default: all)"),
                "style": { "type": "string", "enum": ["none", "Split", "Blinds", "Box", "Wipe", "Dissolve", "Glitter", "R", "Fly", "Push", "Cover", "Uncover", "Fade"] },
                "duration": { "type": "number", "minimum": 0 },
                "dimension": { "type": "string", "enum": ["horizontal", "vertical"] },
                "motion": { "type": "string", "enum": ["inward", "outward"] },
                "direction": { "description": "0, 90, 180, 270, 315 or \"none\"" },
                "scale": { "type": "number", "exclusiveMinimum": 0 },
                "opaque": { "type": "boolean" },
                "display": { "type": "number", "minimum": 0 },
            }),
            &["doc"],
        )),
        t(
            "page_set_box",
            "Set page boxes / crop",
            "Set a page box (crop by default; also trim, bleed, art, media) on pages: either margins in points from the media box [left, bottom, right, top], or an absolute rect in points from the top-left of the displayed page. Omit both to reset the box to its default. Undoable.",
        )
        .cmd("page.boxes")
        .with(schema(
            json!({
                "doc": doc(),
                "pages": pages("to change (default: all)"),
                "box": { "type": "string", "enum": ["crop", "trim", "bleed", "art", "media"] },
                "margins": { "type": "array", "items": { "type": "number", "minimum": 0 }, "minItems": 4, "maxItems": 4 },
                "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
            }),
            &["doc"],
        )),
        t(
            "doc_header_footer",
            "Add header and footer",
            "Add a header and/or footer to pages. Text boxes: header_left/center/right, footer_left/center/right. Tokens: <<1>> page number, <<n>> page count, <<1 of n>>, <<Page 1 of n>>, <<1/n>>, dates <<m/d/yyyy>> <<yyyy-mm-dd>> <<mmmm d, yyyy>> (and more), Bates <<Bates Number#6#1#PREFIX#SUFFIX>>. replace: true swaps out existing ones (Update). Undoable.",
        )
        .cmd("edit.header_footer")
        .with(schema(
            json!({
                "doc": doc(),
                "pages": pages("to mark (default: all)"),
                "header_left": { "type": "string" }, "header_center": { "type": "string" }, "header_right": { "type": "string" },
                "footer_left": { "type": "string" }, "footer_center": { "type": "string" }, "footer_right": { "type": "string" },
                "font_size": { "type": "number", "exclusiveMinimum": 0 },
                "color": { "type": "string", "description": "#RRGGBB or a colour name." },
                "margins": { "type": "array", "items": { "type": "number", "minimum": 0 }, "minItems": 4, "maxItems": 4, "description": "Top, bottom, left, right in points (default 36, 36, 72, 72)." },
                "start_number": { "type": "integer", "minimum": 1 },
                "replace": { "type": "boolean" },
            }),
            &["doc"],
        )),
        t("doc_watermark", "Add watermark", "Add a watermark to pages: text, or a picture from `file` (an image, or page `file_page` of a PDF) at `scale` of the page (default 0.5); rotated (degrees counter-clockwise, default 45), semi-transparent (opacity 0–1, default 0.5), text fitted to the page unless font_size is given, on top unless behind: true; moved from the page centre by `offset_x` (right) and `offset_y` (up) points (default 0). replace: true swaps out existing ones. Undoable.")
            .cmd("edit.watermark")
            .with(schema(
                json!({
                    "doc": doc(),
                    "pages": pages("to mark (default: all)"),
                    "text": { "type": "string", "minLength": 1 },
                    "file": { "type": "string" },
                    "file_page": { "type": "integer", "minimum": 1 },
                    "scale": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 },
                    "font_size": { "type": "number", "exclusiveMinimum": 0 },
                    "color": { "type": "string" },
                    "opacity": { "type": "number", "minimum": 0, "maximum": 1 },
                    "rotation": { "type": "number" },
                    "behind": { "type": "boolean" },
                    "offset_x": { "type": "number", "description": "Points right of the page centre (negative: left)." },
                    "offset_y": { "type": "number", "description": "Points above the page centre (negative: below)." },
                    "replace": { "type": "boolean" },
                }),
                &["doc"],
            )),
        t("doc_background", "Add background", "Fill page backgrounds with a colour, or a picture from `file` (an image, or page `file_page` of a PDF) fitted at `scale` (default 1), behind the content. Undoable.")
            .cmd("edit.background")
            .with(schema(
                json!({ "doc": doc(), "pages": pages("to fill (default: all)"), "color": { "type": "string" }, "file": { "type": "string" }, "file_page": { "type": "integer", "minimum": 1 }, "scale": { "type": "number", "exclusiveMinimum": 0, "maximum": 1 }, "opacity": { "type": "number", "minimum": 0, "maximum": 1 }, "replace": { "type": "boolean" } }),
                &["doc"],
            )),
        t("doc_remove_marks", "Remove header & footer, watermark or background", "Remove every header and footer, watermark or background PdfCraft (or a compatible tool) added. Undoable.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "kind": { "type": "string", "enum": ["header_footer", "watermark", "background"] } }), &["doc", "kind"])),
        t("doc_export_images", "Export pages as images", "Write pages as PNG, JPEG or TIFF files (`<name>_page_<n>.png|jpg|tif`) into a folder, at a resolution (default 150 dpi). JPEG and TIFF are flattened onto white paper. A page too large for the renderer at that resolution is drawn at the most it allows, listed in `lower_dpi` ({page, dpi}). Includes unsaved edits.")
            .cmd("export.image")
            .with(schema(
                json!({
                    "doc": doc(),
                    "folder": { "type": "string" },
                    "format": { "type": "string", "enum": ["png", "jpeg", "tiff"] },
                    "quality": { "type": "integer", "minimum": 1, "maximum": 100, "description": "JPEG quality (default 85)." },
                    "dpi": { "type": "number", "minimum": 18, "maximum": 1200 },
                    "pages": pages("to export (default: all)"),
                }),
                &["doc", "folder"],
            )),
        t("accessibility_autotag_preview", "Preview automatic tags", "Propose paragraph and heading tags for visible text blocks on one page. Returns page bounds, tentative roles and reasons. Does not change the PDF or claim accessibility compliance. Images, tables, lists, annotations and existing structure require separate review.")
            .ro()
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 } }), &["doc", "page"])),
        t("accessibility_autotag_apply", "Automatically tag text-only PDF", "Write marked content, a Document structure element, paragraph tags and a parent tree for every text object in an untagged, text-only PDF. Refuses graphics, images, annotations, forms, existing tags and malformed streams before changing anything. Review all resulting roles; this does not certify accessibility. Undoable.")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("accessibility_autotag_form_fields", "Tag form fields", "Write `/Form` tags, widget structure-parent keys, and a parent tree for all page widgets in an untagged AcroForm. Existing tag trees are refused for review. Text, figures, links, and other content are not tagged.")
            .cmd("a11y.autotag_fields")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("accessibility_check", "Check for accessibility", "Run the Accessibility Checker's full check (32 rules in 7 categories: document, page_content, forms, alternate_text, tables, lists, headings). Each rule is passed, failed (with findings and pages), manual (needs a person) or skipped. Findings on locatable figures, annotations and form widgets include bbox_user_space in PDF coordinates. Colour contrast is off unless all is true; rules (ids such as tagged-pdf, figures-alt-text) or categories narrow the check; pages limit the page rules.")
            .ro()
            .cmd("a11y.check")
            .with(schema(
                json!({
                    "doc": doc(),
                    "rules": { "type": "array", "items": { "type": "string" } },
                    "categories": { "type": "array", "items": { "type": "string", "enum": ["document", "page_content", "forms", "alternate_text", "tables", "lists", "headings"] } },
                    "all": { "type": "boolean", "description": "Include rules that are off by default (colour contrast)." },
                    "pages": pages("for the page rules (default: all)"),
                }),
                &["doc"],
            )),
        t("accessibility_find_untagged", "Find untagged content", "Find untagged painted page content, annotations, and form widgets. This focused check does not modify the PDF or claim that its other accessibility rules pass.")
            .ro()
            .cmd("a11y.find_untagged")
            .with(schema(json!({ "doc": doc(), "pages": pages("to search (default: all)") }), &["doc"])),
        t("accessibility_report", "Accessibility report", "Run the full check and write the accessibility report (HTML) to path; returns the results too. Takes the same options as accessibility_check.")
            .destructive()
            .cmd("a11y.report")
            .with(schema(
                json!({
                    "doc": doc(),
                    "path": { "type": "string" },
                    "rules": { "type": "array", "items": { "type": "string" } },
                    "categories": { "type": "array", "items": { "type": "string" } },
                    "all": { "type": "boolean" },
                    "pages": pages("for the page rules (default: all)"),
                }),
                &["doc", "path"],
            )),
        t("accessibility_review_record", "Accessibility review record", "Compare the same accessibility rules on two open versions of a PDF and write a JSON record to path. Includes document SHA-256 hashes, before/after rule statuses and finding counts, changed rules, and rules still needing review. Does not change either PDF or claim accessibility compliance. Open the original and edited copies as separate documents.")
            .destructive()
            .with(schema(
                json!({
                    "before": doc(),
                    "after": doc(),
                    "path": { "type": "string" },
                    "all": { "type": "boolean", "description": "Include rules that are off by default (colour contrast)." },
                }),
                &["before", "after", "path"],
            )),
        t("accessibility_review_save", "Save accessibility review decisions", "Write a local JSON sidecar for an open PDF with rule-level human review decisions. The sidecar records the PDF SHA-256; replacing one tied to different bytes requires replace: true. Does not change the PDF or assert compliance.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" }, "decisions": { "type": "array", "items": { "type": "object", "properties": { "rule": { "type": "string" }, "decision": { "type": "string", "enum": ["accepted", "needs_work", "skipped"] }, "page": { "type": "integer", "minimum": 1 }, "note": { "type": "string" } }, "required": ["rule", "decision"], "additionalProperties": false } }, "replace": { "type": "boolean" } }), &["doc", "path", "decisions"])),
        t("accessibility_review_load", "Open accessibility review decisions", "Read a local review sidecar only when its recorded SHA-256 matches the open PDF. Returns the decisions without changing the PDF.")
            .ro()
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" } }), &["doc", "path"])),
        t("accessibility_review_html", "Accessibility review summary", "Compare two open versions of a PDF and write a self-contained HTML summary of the same evidence as accessibility_review_record: document hashes, outcome counts, rule changes, and rules still requiring review. Does not change either PDF or claim accessibility compliance.")
            .destructive()
            .with(schema(
                json!({
                    "before": doc(),
                    "after": doc(),
                    "path": { "type": "string" },
                    "all": { "type": "boolean", "description": "Include rules that are off by default (colour contrast)." },
                }),
                &["before", "after", "path"],
            )),
        t("accessibility_fix", "Fix an accessibility problem", "Apply the checker's automatic fix for a rule: primary-language (value: the language, e.g. en-US), title (value: the title; default the current title or file name; also shows it in the title bar) or tab-order (every page tabs in structure order). Returns the rule's new status. Undoable.")
            .with(schema(json!({ "doc": doc(), "rule": { "type": "string", "enum": ["primary-language", "title", "tab-order"] }, "value": { "type": "string" } }), &["doc", "rule"])),
        t("accessibility_figures", "List figures", "The tagged figures (Figure elements, through the role map) in document order: figure number, page, alternate text and where the figure is drawn (top-left-origin points).")
            .ro()
            .cmd("a11y.alt_text")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("accessibility_set_alt", "Set alternate text", "Set a figure's alternate text (alt; empty or omitted clears it), or mark it decorative (decorative: true: its content becomes an artifact and the figure leaves the tags). figure is a number from accessibility_figures. Returns the figures. Undoable.")
            .cmd("a11y.alt_text")
            .with(schema(json!({ "doc": doc(), "figure": { "type": "integer", "minimum": 1 }, "alt": { "type": "string" }, "decorative": { "type": "boolean" } }), &["doc", "figure"])),
        t("accessibility_tags", "List structure tags", "List reachable indirect structure tags in document order with PDF object number, generation, nearest indirect parent, depth, page, type, title, actual text, alternate text and language, one page at a time: count is how many tags match, next the offset of the following page (null at the end). types, pages and max_depth keep only some tags. Inline tag dictionaries have no stable object reference and are not listed.")
            .ro()
            .cmd("a11y.tags")
            .with(schema(
                json!({
                    "doc": doc(),
                    "offset": { "type": "integer", "minimum": 0, "description": "Where the page starts, counted from 0 (the previous page's next)" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Tags in the page (default 200)" },
                    "types": { "type": "array", "items": { "type": "string" }, "description": "Only tags of these types, as listed in type (such as H1, H2, Table)" },
                    "pages": { "type": "array", "items": { "type": "integer", "minimum": 1 }, "description": "Only tags on these pages" },
                    "max_depth": { "type": "integer", "minimum": 0, "description": "Only tags at most this deep (0 = top level)" }
                }),
                &["doc"],
            )),
        t("accessibility_tag_content", "List tag content", "List the marked-content references directly owned by one reachable indirect structure tag, including page, MCID and extracted text.")
            .ro()
            .cmd("a11y.tags")
            .with(schema(json!({ "doc": doc(), "object": { "type": "integer", "minimum": 1 }, "generation": { "type": "integer", "minimum": 0, "maximum": 65535 } }), &["doc", "object", "generation"])),
        t("accessibility_set_tag_property", "Edit tag property", "Set or clear one property of a reachable indirect structure tag. Use object and generation from accessibility_tags. Properties: type, title, actual_text, alternate_text, language. Empty or omitted value clears text properties; type requires a nonempty PDF name. Undoable.")
            .with(schema(json!({
                "doc": doc(), "object": { "type": "integer", "minimum": 1 },
                "generation": { "type": "integer", "minimum": 0, "maximum": 65535 },
                "property": { "type": "string", "enum": ["type", "title", "actual_text", "alternate_text", "language"] },
                "value": { "type": "string" },
            }), &["doc", "object", "generation", "property"])),
        t("accessibility_mark_tag_artifact", "Mark tag content as artifact", "Mark a reachable leaf structure tag's direct marked content as an artifact and remove the tag from the reading structure. Use object and generation from accessibility_tags. Tags with child structure elements, no direct marked content, or content not found on its page are rejected. Undoable.")
            .with(schema(json!({
                "doc": doc(), "object": { "type": "integer", "minimum": 1 },
                "generation": { "type": "integer", "minimum": 0, "maximum": 65535 },
            }), &["doc", "object", "generation"])),
        t("accessibility_move_tag", "Move structure tag", "Move a reachable indirect structure tag before or after a direct sibling in their parent's /K array, changing tagged reading order without changing page appearance. Use object and generation values from accessibility_tags. Rejects distinct parents, duplicate references and positions that are already current. Undoable.")
            .with(schema(json!({
                "doc": doc(), "object": { "type": "integer", "minimum": 1 },
                "generation": { "type": "integer", "minimum": 0, "maximum": 65535 },
                "anchor": { "type": "integer", "minimum": 1 },
                "anchor_generation": { "type": "integer", "minimum": 0, "maximum": 65535 },
                "placement": { "type": "string", "enum": ["before", "after"] },
            }), &["doc", "object", "generation", "anchor", "anchor_generation", "placement"])),
        t("form_merge_data", "Merge data files into spreadsheet", "Collect the field values of form data files (FDF, XFDF) or filled-in PDF forms into one CSV file at path: a column per field name, a row per file. Returns the row and column counts.")
            .cmd("form.merge_data")
            .with(schema(json!({ "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 }, "path": { "type": "string" } }), &["paths", "path"])),
        t("js_run", "Run JavaScript", "Run Acrobat JavaScript in the document, as the JavaScript console does (or as push button `field`'s Mouse Up script when field is given; on a laid-out XFA form, `field` runs that button's XFA click script instead, JavaScript or FormCalc, which can add and remove rows and show or hide subforms). The form object model is available: this/getField, event, app, util, console, display, color, and the document-level scripts. Field changes and resetForm are applied as one undoable step; returns the script's alerts, console output, requests (print, page, url, submit) and error.")
            .cmd("tools.js_console")
            .with(schema(
                json!({ "doc": doc(), "script": { "type": "string" }, "field": { "type": "string", "description": "Run as this button's Mouse Up event." } }),
                &["doc", "script"],
            )),
        t("js_document_scripts", "Document JavaScripts", "List the document-level JavaScripts (name and source), which define functions field scripts use.")
            .ro()
            .cmd("tools.document_js")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("js_set_document_script", "Edit a document JavaScript", "Add or replace the document-level JavaScript `name` with `script`, or delete it (script omitted). Undoable.")
            .cmd("tools.document_js")
            .with(schema(json!({ "doc": doc(), "name": { "type": "string", "minLength": 1 }, "script": { "type": "string" } }), &["doc", "name"])),
        t("form_set_script", "Set a field's JavaScript", "Field Properties ▸ Run custom script: set field's JavaScript for event keystroke, format, validate or calculate (the field's actions; calculated fields join the calculation order) or mouse_up (a button's action). Omit script to remove it. The scripts use Acrobat's object model (event.value, event.rc, getField, util.printf, …) and run when the field changes. Undoable.")
            .cmd("form.prepare")
            .with(schema(
                json!({ "doc": doc(), "field": { "type": "string" }, "event": { "type": "string", "enum": ["keystroke", "format", "validate", "calculate", "mouse_up"] }, "script": { "type": "string" } }),
                &["doc", "field", "event"],
            )),
        t("doc_export_office", "Export to Word, PowerPoint, HTML or RTF", "Export a PDF ▸ Word (.docx), PowerPoint (.pptx, one slide per page), HTML (.html, one file with images inline) or RTF (.rtf), chosen by path's extension: text, headings, bold and italic. Word uses borderless inferred tables and positioned editable shapes on pages with artwork, rotated or large text; each source page retains its size. Font matching and vector graphics remain limited. Export reads forms 50 deep and up to 50 000 a page, counting forms the page draws itself. Pages that go past either limit are listed in truncated_pages, with a warning; the text and images of the forms left out are missing.")
            .cmd("export.docx")
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" } }), &["doc", "path"])),
        t("pdfa_verify", "Verify PDF/A", "Standards ▸ Verify PDF/A compliance: the PDF/A-2b or 3b rules the document breaks (ISO 19005 clause, message, page, whether Save as PDF/A can fix it), plus what it declares.")
            .ro()
            .cmd("standards.pdfa")
            .with(schema(json!({ "doc": doc(), "level": { "type": "string", "enum": ["2b", "3b"], "description": "Default 2b." } }), &["doc"])),
        t("print_readiness", "Print readiness", "Verification only. Reports effective image resolution where each image is drawn (not against the page size), page boxes and bleed past the trim, output intents, fonts that are not embedded, and the PDF/A-2b structural result. Does not modify the document, embed fonts, invent a press profile, or check PDF/X.")
            .ro()
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("pdfa_convert", "Save as PDF/A", "Standards ▸ Save as PDF/A: fix what can be fixed for PDF/A-2b or 3b (XMP identification and metadata, an sRGB output intent, forbidden actions and JavaScript, annotation print flags, image interpolation, encryption). Returns what was fixed and what remains (e.g. fonts that aren't embedded). Undoable; save the document to keep it.")
            .cmd("standards.pdfa")
            .with(schema(json!({ "doc": doc(), "level": { "type": "string", "enum": ["2b", "3b"] } }), &["doc"])),
        t("action_list", "List actions", "Action Wizard: the built-in actions (name, description, steps) and every step an action can use (id, label, whether it takes a text argument).")
            .ro()
            .cmd("actions.wizard")
            .with(schema(json!({}), &[])),
        t("action_run", "Run an action", "Action Wizard: run a built-in action (action: its name) or a list of steps ([{step, arg}], ids from action_list) on each PDF in paths, writing the results into folder under the same names. Returns each file's step log or error.")
            .cmd("actions.wizard")
            .with(schema(
                json!({
                    "action": { "type": "string" },
                    "steps": { "type": "array", "items": { "type": "object", "properties": { "step": { "type": "string" }, "arg": { "type": "string" } }, "required": ["step"] } },
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "folder": { "type": "string" },
                }),
                &["paths", "folder"],
            )),
        t("doc_compare", "Compare files", "Compare the text of two open documents: other is the older version, doc the newer. Returns counts and each change (replaced, inserted, deleted) with the old and new text, pages (1-based) and rectangles (points, origin bottom-left).")
            .ro()
            .cmd("doc.compare")
            .with(schema(json!({ "doc": doc(), "other": { "type": "integer", "description": "The older document (from doc_open)." }, "limit": { "type": "integer", "minimum": 1, "description": "List at most this many changes (default 500)." }, "visual": { "type": "boolean", "description": "Also compare how pages look (page n with page n): regions that differ visually." } }), &["doc", "other"])),
        t("doc_compare_report", "Compare report", "Write the compare summary report (a PDF listing every change) to path.")
            .cmd("doc.compare")
            .with(schema(json!({ "doc": doc(), "other": { "type": "integer" }, "path": { "type": "string" } }), &["doc", "other", "path"])),
        t("doc_compare_mark", "Mark differences as comments", "Add the differences from other (older) to doc (newer) as comments in doc: highlights over replaced (blue) and inserted (green) text, notes where text was deleted (red), authored Compare. Undoable.")
            .cmd("doc.compare")
            .with(schema(json!({ "doc": doc(), "other": { "type": "integer" } }), &["doc", "other"])),
        t("form_detect_fields", "Detect form fields", "Prepare a form ▸ automatic field detection: find the blanks a printed form asks to be filled (underscore runs, lines, empty boxes, rows of comb boxes, small squares for check boxes; on scanned pages read from the pixels) and name each field from its label. With add (default true) the fields are created as one undoable step; otherwise they are only proposed. Returns page (1-based), kind, name and rect (points, origin bottom-left); a row of comb boxes is one text field with comb set to its number of cells, created with the comb flag and that character limit.")
            .cmd("form.detect")
            .with(schema(json!({ "doc": doc(), "pages": pages("to look at (default: all)"), "add": { "type": "boolean" } }), &["doc"])),
        t("form_actions", "Field actions", "Field Properties ▸ Actions: the field's action for each trigger (mouse_up, mouse_down, mouse_enter, mouse_exit, on_focus, on_blur).")
            .ro()
            .cmd("form.prepare")
            .with(schema(json!({ "doc": doc(), "field": { "type": "string" } }), &["doc", "field"])),
        t("form_set_actions", "Set field actions", "Field Properties ▸ Actions: replace the field's actions. Each item: trigger (mouse_up, mouse_down, mouse_enter, mouse_exit, on_focus, on_blur) and one of javascript (source), url, reset (field names; [] for all), menu (Print, NextPage, PrevPage, FirstPage, LastPage), page (1-based), show / hide (field names), submit (URL). Triggers not listed lose their action. Undoable.")
            .cmd("form.prepare")
            .with(schema(
                json!({ "doc": doc(), "field": { "type": "string" }, "actions": { "type": "array", "items": { "type": "object", "properties": {
                    "trigger": { "type": "string", "enum": ["mouse_up", "mouse_down", "mouse_enter", "mouse_exit", "on_focus", "on_blur"] },
                    "javascript": { "type": "string" }, "url": { "type": "string" }, "reset": { "type": "array", "items": { "type": "string" } },
                    "menu": { "type": "string" }, "page": { "type": "integer", "minimum": 1 }, "show": { "type": "array", "items": { "type": "string" } },
                    "hide": { "type": "array", "items": { "type": "string" } }, "submit": { "type": "string" } }, "required": ["trigger"] } } }),
                &["doc", "field", "actions"],
            )),
        t("js_enabled", "JavaScript on or off", "Preferences ▸ JavaScript ▸ Enable Acrobat JavaScript: set it with `enabled`, or read it. With JavaScript off, field scripts other than Acrobat's AF calls don't run.")
            .with(schema(json!({ "enabled": { "type": "boolean" } }), &[])),
        t("ocr_recognize", "Recognize text (OCR)", "Scan & OCR ▸ Recognize text: render pages, read the words in them and add them as invisible text over the page image, so scanned pages become searchable and selectable (a searchable image; the image is not changed). Pages that already have text are skipped unless skip_text_pages is false; then pictures on those pages are read too, and recognised words that the page's own text already has (an earlier OCR layer included) are left out and counted as already_on_page. Returns each page's recognised text, word count or why it was skipped. Needs the OCR models (ocr_status). Undoable as one step.")
            .cmd("ocr.recognize")
            .with(schema(
                json!({
                    "doc": doc(),
                    "pages": pages("to recognise (default: all)"),
                    "dpi": { "type": "number", "minimum": 72, "maximum": 600, "description": "Resolution pages are read at (default 300; a scanned page is read at most at its own resolution)." },
                    "language": { "type": "string", "enum": ["en"], "description": "Document language (default en)." },
                    "skip_text_pages": { "type": "boolean", "description": "Leave pages that already contain text alone (default true). false also reads pictures on such pages, without adding their text again." },
                }),
                &["doc"],
            )),
        t("ocr_recognize_files", "Recognize text in multiple files", "Scan & OCR ▸ Recognize text ▸ In multiple files: read every page of each PDF in paths and write the searchable result into folder under the same name (pages that already have text are left alone unless skip_text_pages is false, as in ocr_recognize). Returns, per file, the output path, word count and skipped pages, or the error.")
            .cmd("ocr.recognize_batch")
            .with(schema(
                json!({
                    "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                    "folder": { "type": "string" },
                    "dpi": { "type": "number", "minimum": 72, "maximum": 600 },
                    "language": { "type": "string", "enum": ["en"] },
                    "skip_text_pages": { "type": "boolean", "description": "Leave pages that already contain text alone (default true); see ocr_recognize." },
                }),
                &["paths", "folder"],
            )),
        t("ocr_status", "OCR status", "Whether text recognition is available (its models are installed: release packages include them; a source build fetches them with `cargo xtask models`; PDFCRAFT_MODELS overrides), where it looks for them, and the languages it reads.")
            .ro()
            .cmd("ocr.recognize")
            .with(schema(json!({}), &[])),
        t("chess_detect", "Find chess diagrams", "Chess ▸ Scan diagrams: find the chess diagrams on pages, from embedded images, contours on the rendered page, text in chess diagram fonts and boards drawn as filled squares, joined into one diagram each and numbered in reading order (column by column unless order is row): diagram n is the same diagram in every chess tool. Returns, per page, each diagram's rect (points from the top-left of the displayed page), bbox_user (PDF user space), the detector that placed it and every detector that saw it (image, contour, font, vector), score, dpi_hint, corners when the board is turned or skewed, and whether its position is in the text; truncated when the page had more than max_per_page; notes on anything skipped. Read-only; needs no model.")
            .ro()
            .with(schema(chess_props(false), &["doc"])),
        t("chess_read", "Read chess diagrams", "Chess ▸ Scan diagrams ▸ Read: find the diagrams as chess_detect does and read each into a position. A diagram set in a chess font is read exactly from the text, with no model; the others need the user's piece model (chess_status) and otherwise say unreadable: no_model, with the reason. A reading gives fen, legal and legality (with violations), side_to_move and how it was decided (side_source: MoveNumber for the first bold move number under the diagram in a born-digital PDF, Legality when the position proves it, Default for White assumed), orientation, min_confidence (the weakest square, not the mean), accepted (legal and every square at least 0.80; a reading the decoder repaired never is, and goes to review), uncertain squares with their second reading, repairs, the dpi it was read at, and lichess_url only for a legal position the decoder did not repair. Read-only.")
            .ro()
            .with(schema(chess_props(true), &["doc"])),
        t("chess_diagram_states", "Count chess diagrams by state", "Chess ▸ Diagrams ▸ the counts the panel's chips show (plan A8): find and read the diagrams as chess_read does, put the corrections kept for doc in store in place unless corrections is false, and count each diagram by its state, by the rule the Chess diagrams panel uses: corrected or verified (a correction kept for it, which outranks the reading), read (the reading passed the gate), review (read, with a doubt or a repair), unread (not read, or no piece model) and error (the reading failed). Returns diagrams, to_review (review, unread and error: the panel's review queue), states (a count for each of read, review, verified, corrected, unread, error), pages (each page with a diagram: page, diagrams, states), generation and corrections (the file read and how many it keeps). Read-only.")
            .ro()
            .with(schema(
                {
                    let mut props = chess_props(true);
                    insert_corrections(&mut props);
                    props
                },
                &["doc"],
            )),
        t("chess_read_area", "Read chess text in an area", "Chess ▸ PGN toolbar ▸ OCR area: read the rectangle rect (PDF user space, either corner first) of page with chess OCR, as the capture tool does on a scan: only that area rendered (300 dpi, less for an area over 4096 px a side), the user's chess-glyphs model (chess_status; without it the call fails and says where to put it), the general OCR for the prose when general_ocr (true by default) and its models are installed, the piece letters read in lang (en by default), then the legality repair. Returns text, lines, edits and flags (words the repair changed or doubted), moves (number, text as printed, san in English, rect from the top-left of the displayed page), and alternatives: every glyph's rect, text, margin (1 − p2/p1 of its top two readings: 0 a tie, 1 certain; the app underlines a glyph below 0.5) and its top three choices with their probabilities. With fen or pgn it also plans capturing the text after at (an address as in chess_pgn_edit; the main line's last move by default) in mode (as chess_pgn_edit's capture op), and capture gives the plan as chess_pgn_edit does, with stopped: each move of the chosen place that did not play, the position it was to be played in, and the legal moves near it (near in English SAN, near_as_printed in lang), which are offered and never applied. Refused for a degenerate area (not finite, under 4 pt a side on the page) and when the document's permissions don't allow copying. Read-only: nothing is applied and the document is not changed.")
            .ro()
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1, "description": "The page, 1-based." },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "The area, [x0, y0, x1, y1] in PDF user space (points, y up)." },
                    "lang": { "type": "string", "maxLength": 8, "description": "The language of the piece letters: en (default), pt, de, es, fr, it, ru, nl, pl, cs, hu, da, sv, no, fi, ro, is or ca." },
                    "general_ocr": { "type": "boolean", "description": "Read the prose with the general OCR when its models are installed (default true)." },
                    "models_dir": { "type": "string", "description": "A file path to the folder holding chess-glyphs.onnx/.json (relative to --root when set). Default: PDFCRAFT_MODELS, then beside the executable." },
                    "fen": { "type": "string", "maxLength": 256, "description": "Plan a capture into a game from this position. Not with pgn." },
                    "pgn": { "type": "string", "maxLength": 1048576, "description": "Plan a capture into this game (its first). Not with fen." },
                    "at": { "type": "string", "maxLength": 65536, "description": "The move the text goes after: main:<half-move> or path:<i.j.k>, as in chess_pgn_edit (default: the main line's last move)." },
                    "mode": { "type": "string", "enum": ["auto", "line", "variation", "subvariation", "comment", "comment_before"], "description": "How the text goes in (default auto)." },
                }),
                &["doc", "page", "rect"],
            )),
        t("chess_document", "Recognised chess document", "Chess ▸ Recognised document: read pages into one document of headings, paragraphs of chess text and diagrams in reading order, with the games replayed from the diagram before them (or from the initial position at 1.) into variation trees. Diagrams are found and read as chess_read does (scan settings and models_dir alike). Text comes from the text layer; the piece letters' language is chosen from all the pages unless lang names it. Headings are blocks set larger than the body text. Moves are valid, illegal, ambiguous, out of sequence (flagged and stopping their line, never dropped), not reached, unchecked (before any position) or a caption (the move a \"Position after:\" caption under a diagram names: not played again, it gives the diagram its side to move and number). Returns a summary (pages, lang, headings, paragraphs, diagrams, games, moves and how many are valid, flagged or captions, pages whose text layer needs OCR, notes, the corrections used); a diagram corrected in the corrections folder (chess_correction_set) takes the position a person set, read_by user, unless corrections is false; include adds the document as JSON (document) and the games as PGN (pgn) to the reply, and out / pgn_out write them to files. The document is not changed.")
            .with(schema(
                {
                    let mut props = chess_props(true);
                    if let Some(o) = props.as_object_mut() {
                        o.insert("lang".into(), json!({ "type": "string", "maxLength": 16, "description": "The piece letters' language as a BCP 47 tag (en, de, fr, es, pt, it, nl, ru, …). Default: chosen from the pages." }));
                        o.insert("ocr".into(), json!({ "type": "string", "enum": ["auto", "off", "all"], "description": "Chess OCR of the page image instead of the text layer: auto (default) on pages whose layer is poor or missing, or is another program's invisible OCR over a scan; off (a born-digital book: never, poor pages are listed in needs_ocr); or all pages (a scanned book). Needs the user's chess-glyphs model; the general OCR reads prose when its models are installed. ocr_pages says what it read." }));
                        o.insert("cipher_in".into(), json!({ "type": "string", "description": "A book cipher saved by an earlier run (cipher_out), to start chess OCR from: what the book's figurine fonts or misreadings map to, learned on other pages." }));
                        o.insert("cipher_out".into(), json!({ "type": "string", "description": "Save the book cipher these pages taught (JSON), for later runs on the same book." }));
                        o.insert("include".into(), json!({ "type": "array", "items": { "type": "string", "enum": ["json", "pgn"] }, "description": "Add the document (json) and/or the games as PGN (pgn) to the reply." }));
                        o.insert("out".into(), json!({ "type": "string", "description": "Write the document as JSON to this file." }));
                        o.insert("pgn_out".into(), json!({ "type": "string", "description": "Write the games as PGN to this file." }));
                        o.insert("reference_pgn".into(), json!({ "type": "string", "description": "The book's own PGN (a course's or an e-book's games), at most 32 MiB and 4,096 games: each move of the text is linked to it as confirmed, recovered (the reader could not play it, the PGN did: its status is kept and it gets a from_reference review flag with the PGN's move), conflict (played to a position the PGN does not have) or absent, and each move of the document JSON gets reference {kind, game, node}, its game counting from 0. The reply's reference field sums it up: games, moves, stopped_games, linked counts, diagrams placed on a PGN move, the game each page discusses (pages and games from 1) and warnings, one of them when under 30% of the moves on the first 20 pages read are in the PGN. The text and the games are not changed." }));
                    }
                    insert_corrections(&mut props);
                    props
                },
                &["doc"],
            )),
        t("chess_add_lichess_links", "Add lichess links", "Chess ▸ Add lichess links: under each diagram whose position is known, a small blue label (Helvetica, default \"Analyze on lichess\", 8 pt) with a link to the position's lichess.org analysis board. Diagrams are found and read as chess_read does. A diagram corrected in the corrections folder (chess_correction_set) is linked to the position a person set, unless corrections is false. Only a legal position the decoder did not repair is linked, and by default only a reading that passed the gate (allow_unaccepted also takes legal readings with an uncertain square); a page that already links to the same position is skipped. The label goes below the diagram, else to its right, else above it, clear of the page edge and of other diagrams and labels. Returns the links added (page, index, fen, url, side, rect from the top-left of the displayed page, link_user) and the diagrams skipped with the reason. One undo step; the label is added text that can be moved or edited later. dry_run changes nothing. Nothing is fetched: the link opens when a reader clicks it. Needs permission to change the document.")
            .with(schema(
                {
                    let mut props = chess_props(true);
                    if let Some(o) = props.as_object_mut() {
                        o.insert("label".into(), json!({ "type": "string", "maxLength": 80, "description": "The label's text, one line Helvetica can draw (WinAnsi). Default \"Analyze on lichess\"." }));
                        o.insert("size".into(), json!({ "type": "number", "minimum": 4, "maximum": 48, "description": "The label's size in points (default 8)." }));
                        o.insert("allow_unaccepted".into(), json!({ "type": "boolean", "description": "Also link legal readings that did not pass the reading gate (default false). Repaired readings are never linked." }));
                        o.insert("dry_run".into(), json!({ "type": "boolean", "description": "Only say what would be done; change nothing (default false)." }));
                    }
                    insert_corrections(&mut props);
                    props
                },
                &["doc"],
            )),
        t("chess_export_html", "Export chess document as HTML", "Chess ▸ Export ▸ HTML: read pages as chess_document does and write one static HTML file to out (no script, nothing fetched): headings, paragraphs with moves (figurines: unicode ♞f3 by default, letters in letters_lang, or as printed), annotation glyphs, and each diagram redrawn as an inline SVG board (diagrams: svg) or its FEN (fen), with a link to the position on lichess under it (links, default true; link_text). Pieces come from PDFCRAFT_CHESS_PIECES, a pieces folder beside the executable or an installed Chess Merida, else plain geometric ones (pieces: geometric forces them). Figurines and annotation signs use the fonts named by PDFCRAFT_CHESS_TEXT_FONT or found in the fonts folder beside the executable: a figurine font by letters (SemFig) or with Unicode chess symbols, and ISChess for the signs (text_font and sign_font: path, family, role, format, used, error). Readable back: each diagram is a figure with data-fen, each move a span with data-san (and data-fen after it), a diagram that was not read is a figure with class unread; anything to review has class review. Returns out, bytes, pages, lang, diagrams (drawn, unread), games, the piece set used, pages needing OCR, notes and the corrections used: a diagram corrected in the corrections folder (chess_correction_set) is drawn, linked and played from the position a person set, unless corrections is false. The document is not changed.")
            .with(chess_export_schema()),
        t("chess_export_epub", "Export chess document as EPUB", "Chess ▸ Export ▸ EPUB: the chess_export_html document as an EPUB 3 book at out: one XHTML page per PDF page (the same markup and inline SVG boards), a table of contents from the headings (or the pages), the package document with a stable identifier made from the PDF's bytes and the date, and a cover image drawn from the first page (at most 1600 px on its long side; when it cannot be drawn the book has none and the reply's notes say so). No script, nothing fetched. The user's figurine and sign fonts go in the package when found (one whose licence forbids embedding, OS/2 fsType Restricted, is refused and text_font says why); the package says how it can be read (schema:accessMode, accessibilityFeature, accessibilityHazard, accessibilitySummary), dc:creator (the PDF's author) and dc:source (its file name); every page starts with a page break named by its printed number, listed in the navigation's page list; the pages share one styles.css; the identifier comes from the PDF's /ID when it has one. Same options and reply as chess_export_html. The document is not changed.")
            .with(chess_export_schema()),
        t("chess_export_docx", "Export chess document as Word", "Chess ▸ Export ▸ Word: the chess_export_html document as a .docx file at out: a flowing document in the book's language (w:lang): Word headings, paragraphs whose moves and annotation glyphs spelling checks skip (w:noProof), piece letters and signs in the user's figurine font (SemFig) and sign font (ISChess) when found, as chess_export_html sets them (named, not embedded: the reply's notes say they must be installed where the file is read), and each diagram as a PNG picture (the same board as the PDF replacement, diagram_px pixels a side for 3 inches) whose text alternative (wp:docPr descr) says the position in words, with a caption in Word's Caption style kept on its page: the PGN's comment for it (Page 17 · diagram 1 of 2), its FEN and, with links, its lichess address as a real hyperlink (only http/https addresses become links). A diagram that was not read is said in words. Same options and reply as chess_export_html (diagrams does not apply). The document is not changed.")
            .with(chess_export_schema()),
        t("chess_export_rtf", "Export chess document as RTF", "Chess ▸ Export ▸ RTF: as chess_export_docx, written as an .rtf file at out (figurines as \\uN? escapes, moves \\noproof, diagrams as PNG pictures whose wzDescription is their text alternative); its font table lists every font a run names.")
            .with(chess_export_schema()),
        t("chess_export_markdown", "Export chess document as Markdown", "Chess ▸ Export ▸ Markdown: the chess_export_html document as a .md file at out: # headings, paragraphs with moves (figurines as chess_export_html; text escaped so it stays text), and each diagram as an image whose alt text says it in the book's language (Diagram 1, page 17: White to move. White: Kg1…; with the printed page number) and whose title is its FEN, ![…](<name>_diagrams/diagram-pNNNN-k.png \"FEN\"), written as PNG into a folder beside the file (its name with what Windows does not allow made _; diagrams: fen sets the FEN in backticks and writes no images), with its lichess link under it. A front matter gives title and lang, and a rule comes before each chapter (each heading of the book's top level), not at each page. Same options and reply as chess_export_html, plus image_dir (the folder, relative to out). The document is not changed.")
            .with(chess_export_schema()),
        t("chess_export_pgn", "Export chess games as PGN", "Chess ▸ Export ▸ PGN: every game found in the text (roster, SetUp/FEN from the diagram it starts at, NAGs, comments, variations), for each legal, unrepaired diagram positions asks for a game with no moves holding its position (unused, the default: those no game starts from or reprints; all; none), and $220 on the move whose position a diagram reprints, all with PdfPage (the physical page, from 1) and PdfDiagram tags, PdfPageLabel when the page's printed number differs, SourceTitle (the export title), and a first comment naming the page and diagram in the book's language (Page xvii · diagram 1 of 2); a position's game has the title as its Event and Round -; in the order of the book, written to out. A position whose reading did not pass review says so in a comment. Reply as chess_export_html, plus position_games (how many such games were added) and positions_skipped (each diagram positions asked a game for that got none: page from 1, diagram from 1, reason not_read, illegal or repaired). variations, comments and nags leave things out of the moves (all kept by default; the tags are always written). The document is not changed.")
            .with(chess_export_pgn_schema()),
        t("chess_status", "Chess model status", "Whether the user's chess models are available (none ship with PdfCraft): the piece model reads drawn and scanned diagrams, the glyph model chess text. Each is loaded, missing or invalid (with the reason). models_dir looks in that folder instead of PDFCRAFT_MODELS and beside the executable. Also says where the piece set for drawing diagrams comes from (PDFCRAFT_CHESS_PIECES, a pieces folder of images beside the executable, an installed Chess Merida, or plain geometric pieces). Diagrams in chess fonts are read without any model.")
            .ro()
            .with(schema(json!({ "models_dir": models_dir() }), &[])),
        t("chess_pieces", "Chess piece sets", "The piece sets that can be chosen on this machine, as Preferences ▸ Chess pieces lists them: automatic, the sets PdfCraft ships (with author and licence), diagram fonts in the fonts folder beside the executable and an installed Chess Merida, folders of piece images in the pieces folder beside the executable (a folder missing pieces says which), and plain shapes. Each has its preferences token and, where the chess tools take it, the value of their pieces argument. environment says whether PDFCRAFT_CHESS_PIECES is set, which takes precedence over any choice.")
            .ro()
            .with(schema(json!({}), &[])),
        t("chess_pieces_import", "Import chess piece sets", "Preferences ▸ Chess pieces ▸ Import: copies a folder of twelve piece images (wK to bP, SVG or PNG) or the piece sets a saved web page carries inside it (data:image/svg+xml values keyed wK to bP, plain or base64; a page that fetches its pieces from the web has none) into into, one folder per set (default: the pieces folder beside the executable; needed with --root), each with an ORIGIN.txt saying where it came from and that its licence is unknown, for personal use only. Every piece is read first (an SVG may not point at another file) and nothing is written unless all read. A set of that name already there is replaced only with replace; otherwise the reply lists it under exists and imports nothing. Returns imported (name, folder and the preferences token of each).")
            .with(schema(
                json!({
                    "from": path_arg(),
                    "into": { "type": "string", "description": "The folder to import into (relative to --root when set)." },
                    "replace": { "type": "boolean", "description": "Replace sets of the same name already in into (default false)." },
                }),
                &["from"],
            )),
        t("chess_game", "Play through a chess game", "Chess ▸ Board without the interface: set up a game from fen (a position; the standard start without it) or pgn (its first game: tags, moves, variations, comments and glyphs; a FEN tag gives its start), play moves (SAN as books print it, or UCI such as e2e4 and e7e8q) after the end of its main line, then show the position at ply (moves from the start along the main line; default its end). An illegal move stops the moves there and is returned as stopped (the first one, in pgn or moves). Returns the position shown (fen, side_to_move, in_check, outcome: ongoing, checkmate, stalemate or fifty_moves, legal_moves in SAN), start_fen, ply, plies (the main line's length), movetext, the game as pgn, and header: what the move list shows above and after the moves (white, white_elo, black, black_elo, event, site, date as far as it is known, round, each null when its tag says nothing, as PGN's ?; and result: 1-0, 0-1, 1/2-1/2, or * while the game goes on). No document is read or changed.")
            .ro()
            .with(schema(
                json!({
                    "fen": { "type": "string", "maxLength": 256, "description": "The start position. Not with pgn." },
                    "pgn": { "type": "string", "maxLength": 1048576, "description": "A game in PGN. Not with fen." },
                    "moves": { "type": "array", "maxItems": 2000, "items": { "type": "string", "maxLength": 32 }, "description": "Moves to play after the main line's end, in SAN or UCI." },
                    "ply": { "type": "integer", "minimum": 0, "description": "Show the position this many moves into the main line." },
                }),
                &[],
            )),
        t("chess_pgn_edit", "Edit a chess game's moves", "Chess ▸ Board ▸ moves, without the interface: start from game (0-based, 0 by default) of pgn, which may hold several (at most 256, cut as chess_games cuts them; an index past the last is refused), or from fen (the standard start without either) and apply ops in order, each at an address: main:<n> is the main-line move after which the absolute half-move is n (2·(move number−1), plus 1 when Black is to move; the start's own when nothing was played, so main:0 from the standard start and main:35 from a FEN \"… b - - 0 18\"), path:<i.j.k> the move reached by those reply indices from the start (0 continues the line, 1 is the first variation; path: is the start). Ops: play {at, moves: SAN or UCI, a reply already there is followed}; comment {at, text} and comment_before {at, text} (set, at most 4096 bytes); nag {at, code} (set the PGN code as given; ↑ after a Black move is 37); play_null {at} (a null move, written --: the side to move passes, to show the other side's threat; refused in check), move_variation {at, to: up or down} (a variation changes places with the one before or after it at its fork; the line itself never moves), diagram {at, on? (true by default)} (mark the move for a diagram, $220, or take the mark off); promote_one {at} (a variation up one place), make_main {at} (its line becomes the main line), demote {at} (a first reply down to second), delete_variation {at} (a variation and its moves), cut {at} (every move after at); back {at?, n? (1 by default)} and forward {at?, n?} (step through the game from at, the move shown without it: forward retraces the moves last stepped back over, into a variation when that is where they were, and otherwise follows the first reply; the entry says how many moves it moved); choose {at?, index} (step forward into reply index: 0 continues the line, 1 is its first variation; remembered as the line forward follows); next_variation {at?} and prev_variation {at?} (go to the first move of the next or previous variation in reading order, the order the notation writes them, into and out of variations and across the main line; prev from inside a variation goes to its first move; the entry says whether it moved); graft {at, mode: follow (as a variation, the default), line (a new move becomes the first reply and demotes the old one) or comment (the items as text on at), items: [{item: move, san (English SAN), ply?, bold?} | {item: nag, code (White's; the side's code is applied)} | {item: comment, text, before?} | {item: open} | {item: close} | {item: result}]}, which reports what was played, where it stopped and why, the moves added, parentheses that did not balance, glyphs and comments with no place, and the leftover text; capture {at (the current move), text (a selection of move text as printed: numbers, prose, brackets), lang? (en by default), mode? (auto, the default: bold first move = the main line, else a variation of the nearest line that takes it; line; variation (of the main line); subvariation; comment; comment_before), bold_runs? ([start, end) character ranges of text in bold), apply? (chosen, the default; auto: only when no preview is needed; never; renumber: the Renumber choice)}, which reads the text from every place it could go (on the current move's line and each line outward, at the move its first number names), scores each by the moves played, and reports every candidate (anchor, line, mode, played, stops, reasons, movetext), the chosen one, renumber (the index of the Renumber choice, not a place: when the first printed number names no place where a move without one goes, the text read from there with every number moved alike, so its moves take the game's numbers, its candidate saying shifted (moves) and renumbering {printed, played}; a game with no moves at move 1 takes the text's number at its start instead; chosen only when no place plays a move), needs_preview with its reasons (ambiguous, incomplete, outer_line, tip_extension, renumber, implied_close, demotes, no_fit, too_long) and whether it was applied. Each op adds a report entry with ok and path (the move shown after it); a refused op adds ok: false with the error, changes nothing and stops the list. Returns pgn (the game edited only) and movetext, written as variations, comments and nags say (everything by default), report, applied (the ops done), current_path, games (how many games pgn holds; 0 without pgn), game (the one edited), pgn_stopped (the first move of that game that could not be played, whose line ends there) and pgn_warnings (each variation of pgn kept as a comment where it stood instead of read: kind orphan_variation, with no move before it to replace, or too_deep, nested past 32 levels; at, the address of the move it stood after). No document or file is read or changed.")
            .ro()
            .with(schema(
                {
                    let mut props = json!({
                    "fen": { "type": "string", "maxLength": 256, "description": "The start position. Not with pgn." },
                    "pgn": { "type": "string", "maxLength": 1048576, "description": "One game or several in PGN. Not with fen." },
                    "game": { "type": "integer", "minimum": 0, "description": "The game of pgn to edit, 0-based (the first by default). Only with pgn." },
                    "ops": {
                        "type": "array",
                        "maxItems": crate::chess_pgn::MAX_OPS,
                        "description": "The operations, applied in order. 20,000 moves and items at most in all.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": { "type": "string", "enum": ["play", "comment", "comment_before", "nag", "play_null", "move_variation", "diagram", "promote_one", "make_main", "demote", "delete_variation", "cut", "back", "forward", "choose", "next_variation", "prev_variation", "graft", "capture"] },
                                "at": { "type": "string", "maxLength": 65536, "description": "main:<half-move> or path:<i.j.k>. Optional for back, forward, choose, next_variation and prev_variation (the move shown)." },
                                "n": { "type": "integer", "minimum": 1, "description": "back, forward: how many moves (1 by default)." },
                                "index": { "type": "integer", "minimum": 0, "description": "choose: the reply to step into (0: the line, 1: its first variation…)." },
                                "moves": { "type": "array", "maxItems": 2000, "items": { "type": "string", "maxLength": 32 } },
                                "text": { "type": "string", "maxLength": 65536, "description": "A comment (4096 bytes at most), or the text a capture reads (65536)." },
                                "code": { "type": "integer", "minimum": 1, "maximum": 255 },
                                "mode": { "type": "string", "enum": ["follow", "line", "comment", "auto", "variation", "subvariation", "comment_before"], "description": "graft: follow, line or comment. capture: auto, line, variation, subvariation, comment or comment_before." },
                                "lang": { "type": "string", "maxLength": 16, "description": "capture: the language the text's piece letters are in (en, pt, de, …). English by default." },
                                "bold_runs": {
                                    "type": "array",
                                    "maxItems": crate::chess_pgn::MAX_BOLD_RUNS,
                                    "description": "capture: the character ranges [start, end) of text set in bold.",
                                    "items": { "type": "array", "minItems": 2, "maxItems": 2, "items": { "type": "integer", "minimum": 0 } },
                                },
                                "apply": { "type": "string", "enum": ["chosen", "auto", "never", "renumber"], "description": "capture: apply the chosen place (the default), only when no preview is needed (auto), never, or the Renumber choice (renumber: the moves take the game's numbers)." },
                                "to": { "type": "string", "enum": ["up", "down"], "description": "move_variation: which way the variation moves." },
                                "on": { "type": "boolean", "description": "diagram: mark the move (true, the default) or take the mark off." },
                                "items": {
                                    "type": "array",
                                    "maxItems": 4096,
                                    "items": {
                                        "type": "object",
                                        "properties": {
                                            "item": { "type": "string", "enum": ["move", "nag", "comment", "open", "close", "result"] },
                                            "san": { "type": "string", "maxLength": 32 },
                                            "ply": { "type": "integer", "minimum": 0 },
                                            "bold": { "type": "boolean" },
                                            "code": { "type": "integer", "minimum": 1, "maximum": 255 },
                                            "text": { "type": "string" },
                                            "before": { "type": "boolean" },
                                        },
                                        "required": ["item"],
                                    },
                                },
                            },
                            "required": ["op"],
                        },
                    },
                });
                    if let Some(o) = props.as_object_mut() {
                        for (name, prop) in pgn_style_props() {
                            o.insert(name.into(), prop);
                        }
                    }
                    props
                },
                &["ops"],
            )),
        t("chess_glyph_sample", "Save a corrected chess character for retraining", "Chess ▸ PGN toolbar ▸ Collect corrected characters for retraining, without the interface: render the box rect of page again (from the top-left of the displayed page, in points, as chess_read_area's alternatives give a character's box; at most 72 pt a side) at dpi (300 by default, kept between 100 and 300), padded with white to a square and scaled to a 32 x 32 gray PNG (the picture the app saves), and write it as <folder>/<class folder>/<confidence in thousandths, 4 digits>_<16-digit hash of the document's bytes>_p<page, 4 digits>_<n>.png, the layout of ChessVisionOFF's review queue (revisao_ocr): the class folder is text's class as ChessVisionOFF names it (upper_A, lower_a, digit_1, sym_<code point> for any other character such as sym_9816 for a knight figurine, ligature_<text> or ligature_hex_<code points> for several), n counts up from 1 past the files there (nothing is overwritten), and the file is written atomically. text is the corrected reading (1 to 16 characters); confidence is the model's probability for what it had read, 0 to 1 (0 by default). Refused for a document that does not allow copying, and for an encrypted one unless allow_encrypted (the sample is written unencrypted). folder is confined to --root, and so is the file finally written. One session (one server or script run) writes at most 10,000 samples and 16 MiB of them; a call past either is refused and writes nothing. Returns path, class, name, side (32) and bytes. Nothing is trained and the document is not changed.")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4, "description": "[x0, y0, x1, y1]: the character's box in points from the top-left of the displayed page." },
                    "text": { "type": "string", "minLength": 1, "maxLength": 64, "description": "What the character is (the corrected reading), 1 to 16 characters." },
                    "confidence": { "type": "number", "minimum": 0, "maximum": 1, "description": "The model's probability for its first reading (default 0): the most doubtful sort first." },
                    "dpi": { "type": "number", "minimum": 72, "maximum": 600, "description": "The resolution the area was read at (default 300; kept between 100 and 300)." },
                    "folder": { "type": "string", "description": "A file path to the folder of samples, the review queue's root (relative to --root when set)." },
                    "allow_encrypted": { "type": "boolean", "description": "Write a sample of an encrypted document (unencrypted). Default false." },
                }),
                &["doc", "page", "rect", "text", "folder"],
            )),
        t("chess_correction_list", "List a document's chess diagram corrections", "Chess ▸ Diagrams ▸ corrections kept on disk, without the interface (plan A8): the corrections of doc in the folder store (confined to --root; .pdfcraft/chess-corrections by default), one JSON file per document named by a hash of its keys and found again by its first file identifier, the hash of its path or its name and size, so saving, renaming or moving it keeps them. Each correction is found in the document as it is now by its page's fingerprint (a hash of the page's content streams and images as stored, which saving keeps): located is same (its page, unchanged), moved (pages were inserted or removed before it; page is where it is now), changed (its page by number, whose content changed) or orphan (its page is gone; page null). Returns file, read_only (a file of a newer version, never written), set_aside (a file that could not be read, renamed to …bad, never deleted, and why), count, orphans and corrections (uid, page 1-based now, located, recorded_page, rect: the diagram's box as fractions of the crop box [x0, y0, x1, y1], y up; fen, kind: corrected or verified, original_fen: the reading it replaced, at: seconds since 1970, rev). Only doc's corrections are listed; the file keeps no text or picture of the document and its path only as a hash. Nothing is changed.")
            .with(schema(json!({ "doc": doc(), "store": { "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." }, }), &["doc"])),
        t("chess_correction_set", "Keep a chess diagram's correction", "Chess ▸ Diagrams ▸ keep a correction on disk, without the interface (plan A8): scan page of doc for its diagrams (read too, with models_dir as chess_read takes it), and keep diagram index (1-based, in reading order) as kind: corrected (fen, the corrected position, required) or verified (the reading was right: fen defaults to the reading's position; refused for a diagram that was not read and no fen). The correction is the diagram's box as fractions of the page's crop box, the page and its fingerprint, the position and the reading it replaced, in the folder store (confined to --root; .pdfcraft/chess-corrections by default). A diagram already corrected (the same page, boxes overlapping by an intersection over union of 0.7 or more) gets its next revision, keeping its uid; another gets a new uid. The file is read, changed and written under a lock in the folder, through a staging file renamed over it, so two writers keep both changes; a file of a newer version is refused and left as it is. Returns uid, page, index, rect, fen, kind, legal (whether the position can start a game), rev and file. The document is not changed.")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "index": { "type": "integer", "minimum": 1, "description": "The diagram on the page, 1-based in reading order (as chess_detect lists them)." },
                    "fen": { "type": "string", "maxLength": 512, "description": "The position (a FEN; a placement alone means White to move). Required for corrected." },
                    "kind": { "type": "string", "enum": ["corrected", "verified"], "description": "corrected (default) or verified: the reading was right." },
                    "models_dir": { "type": "string", "description": "A folder with the piece model, as chess_read takes it (relative to --root when set)." },
                    "store": { "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." },
                }),
                &["doc", "page", "index"],
            )),
        t("chess_page_verify", "Mark a page's chess diagrams as checked", "Chess ▸ Diagrams ▸ Mark the page as checked, without the interface (plan A8): scan page of doc and read its diagrams (models_dir as chess_read takes it), and keep every diagram read with confidence (its reading passed the gate) that has no correction yet as verified, with its reading's position, in the folder store (confined to --root; .pdfcraft/chess-corrections by default), as chess_correction_set kind verified would. The others are left for a person to check one by one, never accepted in bulk: kept (a correction is kept for it already), review (read with a doubt or a repair), unread (not read, or no piece model) or error. Returns page, verified (index, uid, fen, rev), left (index, why) and file. The document is not changed.")
            .with(schema(
                {
                    let mut props = chess_props(true);
                    if let Some(o) = props.as_object_mut() {
                        o.remove("pages");
                        o.insert("page".into(), json!({ "type": "integer", "minimum": 1, "description": "The page whose diagrams to mark as checked." }));
                        o.insert("store".into(), json!({ "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." }));
                    }
                    props
                },
                &["doc", "page"],
            )),
        t("chess_correction_delete", "Delete a chess diagram's correction", "Chess ▸ Diagrams ▸ delete a correction kept on disk, without the interface (plan A8): mark correction uid of doc deleted in the folder store (confined to --root; .pdfcraft/chess-corrections by default), leaving a tombstone with the next revision so a copy merged later never brings it back. Refused for a uid doc's corrections don't have live (see chess_correction_list) and for a file of a newer version. Returns uid, deleted and file. The document is not changed.")
            .with(schema(json!({ "doc": doc(), "uid": { "type": "string", "maxLength": 512 }, "store": { "type": "string", "description": "A file path to the corrections folder, one file per document (relative to --root when set; default .pdfcraft/chess-corrections)." }, }), &["doc", "uid"])),
        t("chess_games", "Edit a collection of chess games", "Chess ▸ Board ▸ Games in this document, without the interface: start from a collection of games in PGN, inline in pgn or read from the .pgn file path (not both; one blank game without either; at most 256 games and 32 MiB, split at the tag line that follows moves; a game that cannot be read at all is refused, never left out), game active shown (0-based, the first by default), and apply ops in order, as the app's list does: list (the games); set_active {game}; rename {game?, white?, black?, event?, site?, date?, round?} (sets those tags, so the name is in the PGN; an empty value removes the tag; the others are kept; date is YYYY.MM.DD with ? for each unknown digit, any other is refused); duplicate {game?} (a copy right after it, shown); delete {game?} (the game shown stays shown; deleting it shows the one after it, the one before at the end; deleting the last game leaves a blank one); move {game?, to} (to its place once moved, any place; the game shown stays shown); new_from {game?, at?} (a game starting at the position of move at of that game, an address as in chess_pgn_edit, the main line's last move by default, with SetUp/FEN tags, right after it and shown); new_at {game?, fen} (a game starting at position fen, as the board's Set up a position makes one over a game with moves: right after game, which keeps its moves, and shown); export {game?} (that game's PGN, or the whole collection's). game is 0-based and defaults to the game shown. Each op adds a report entry with ok and active (the game shown after it); a refused op adds ok: false with the error, changes nothing and stops the list. Returns games (index, name: White–Black, else the player, else the event, or null; white, black, event, site, date, round, moves counted as a scoresheet does, start_fen, shown, pgn_stopped: the first move of its PGN that could not be played), active, count, pgn (every game in list order, a blank line between two), report, applied, and written: the file out the collection was written to (atomically; only when every op was applied), else null. The app's own autosaved collections are not read or changed.")
            .with(schema(
                json!({
                    "pgn": { "type": "string", "maxLength": 33554432, "description": "The games in PGN. Not with path." },
                    "path": { "type": "string", "description": "A file path to a .pgn file to read the games from (relative to --root when set). Not with pgn." },
                    "active": { "type": "integer", "minimum": 0, "maximum": 255, "description": "The game shown at the start (0-based; default 0)." },
                    "out": { "type": "string", "description": "A file path to write the collection to as PGN, in list order, once every op was applied (relative to --root when set). Omit to only return it." },
                    "ops": {
                        "type": "array",
                        "maxItems": crate::chess_games::MAX_OPS,
                        "description": "The operations, applied in order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "op": { "type": "string", "enum": ["list", "set_active", "rename", "duplicate", "delete", "move", "new_from", "new_at", "export"] },
                                "game": { "type": "integer", "minimum": 0, "maximum": 255, "description": "The game acted on (0-based). Default: the game shown (export: every game)." },
                                "to": { "type": "integer", "minimum": 0, "maximum": 255, "description": "move: the game's place once moved (0-based)." },
                                "at": { "type": "string", "maxLength": 65536, "description": "new_from: main:<half-move> or path:<i.j.k> in the game (default: its main line's last move)." },
                                "fen": { "type": "string", "maxLength": 256, "description": "new_at: the position the new game starts at." },
                                "white": { "type": "string", "maxLength": 1024, "description": "rename: the White tag (empty removes it)." },
                                "black": { "type": "string", "maxLength": 1024, "description": "rename: the Black tag (empty removes it)." },
                                "event": { "type": "string", "maxLength": 1024, "description": "rename: the Event tag (empty removes it)." },
                            },
                            "required": ["op"],
                        },
                    },
                }),
                &[],
            )),
        t("content_draw", "Draw content operators", "Draw a fragment of PDF content-stream operators (ISO 32000-2 §8–9) on top of a page, in its default user space (points, y up, origin at the media box's corner; /Rotate is not applied), as page content of its own wrapped in q … Q. Checked first and refused with the reason: at most 4 MiB; q/Q, BT/ET and marked content balanced (q at most 28 deep); operand counts and finite numbers; only path, clipping, device colour (g, rg, k, and cs/CS with DeviceGray, DeviceRGB or DeviceCMYK), general graphics state, text and marked-content operators with inline property lists; nothing that needs a page resource (Do, gs, sh, inline images, patterns). Text may use /PCHelv, /PCTimes or /PCCour (standard fonts, WinAnsi), which are added for it. One undo step.")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "ops": { "type": "string", "maxLength": 4194304, "description": "The operators, e.g. \"1 0 0 rg 72 72 144 72 re f\"." },
                }),
                &["doc", "page", "ops"],
            )),
        t("chess_replace_diagram", "Replace a chess diagram", "Chess ▸ Replace diagram: draw a new board, as vector paths, over diagram index of page (numbered as chess_detect numbers them, with the same scan settings; generation is the one chess_detect reported, and a document changed since is refused). The board shows upright as the page is displayed, as large as fits in the diagram's box and centred. The old diagram: an embedded image that is the diagram is deleted; text in a chess diagram font is removed and its box covered; anything else (a scanned page, squares drawn as paths, an image that also holds a caption) is covered with opaque white and stays in the page's content underneath. With neither fen nor placement, the position kept for the diagram in the corrections folder store is drawn (Redraw in the PDF; from_correction gives its uid), and keep_correction keeps the position drawn as the diagram's correction with the page as it is after the drawing (correction: uid, kind, rev, file), so the correction follows the page. Returns what was done (old_diagram: image_deleted, text_removed_and_covered or covered), the diagram's box, the cover and the board's box (user space; rect and board_rect from the top-left of the displayed page), the piece set used, and notes on what stays. One undo step; dry_run changes nothing. Needs permission to change the document.")
            .destructive()
            .with(schema(chess_replace_props(), &["doc", "page", "index", "generation"])),
        t("doc_export_all_images", "Export all images", "Write the images that pages use into a folder (`<name>_Page_<n>_Image_<k>.jpg|png`, n zero-padded to the page count's digits), each once: JPEG images unchanged, others as PNG with their soft mask as alpha. min_size skips images with fewer pixels on their shorter side. Images that can't be decoded yet (JPEG 2000, JBIG2, CCITT, separations) are listed under skipped. Includes unsaved edits.")
            .cmd("export.all_images")
            .with(schema(
                json!({
                    "doc": doc(),
                    "folder": { "type": "string" },
                    "pages": pages("whose images to export (default: all)"),
                    "min_size": { "type": "integer", "minimum": 0, "description": "Skip images smaller than this many pixels on their shorter side (default 0)." },
                }),
                &["doc", "folder"],
            )),
        t("doc_export_text", "Export text", "Write the reading-order text of pages to a .txt file (pages separated by form feeds).")
            .cmd("export.text")
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" }, "pages": pages("to export (default: all)") }), &["doc", "path"])),
        t(
            "fill_sign_add",
            "Fill & Sign: type text or place a mark",
            "Fill in a form that has no fields, as Acrobat's Fill & Sign does: type text (`text`, default 10 pt; `font_size` 4–300 pt), place a check, cross, dot or line, today's date (in Preferences ▸ Date format, see fill_sign_date_format, or `format`; `font_size` also applies), or a signature or initials (either `text` drawn in a script font, or `path` to a local PNG/JPEG image up to 4 MiB and 32 megapixels, scaled down to 4 megapixels, preserving transparency; with `scan: true`, `path` is instead a photo of a signature on paper, within the same limits: the signature is found, and the paper, shadows and table around it are removed, with `sensitivity` 0–100, default 50, picking up fainter ink when higher). `at` [x, y] is in points from the top-left of the page: text's top-left, mark's centre, signature's left edge centred vertically. Image signatures fit within 150 pt wide and 32 pt tall (24 pt for initials). Creates movable, undoable annotations. Typed text the standard fonts can't draw is saved and drawn as `?` (`warnings` in the result says so); a typed signature neither the script font nor its fallback can draw is refused (pass `path` instead).",
        )
        .cmd("sign.fill.text")
        .with(schema(
            json!({
                "doc": doc(),
                "page": { "type": "integer", "minimum": 1 },
                "type": { "type": "string", "enum": ["text", "check", "cross", "dot", "line", "date", "signature", "initials"] },
                "at": point(),
                "text": { "type": "string", "minLength": 1 },
                "font_size": { "type": "number", "minimum": 4, "maximum": 300, "description": "For type text or date (default 10 pt)." },
                "path": { "type": "string", "description": "PNG or JPEG for signature/initials; pass either path or text." },
                "scan": { "type": "boolean", "description": "With path: scan a photo of a signature on paper (default false)." },
                "sensitivity": { "type": "integer", "minimum": 0, "maximum": 100, "description": "With scan: how faint ink may be, 0–100 (default 50)." },
                "format": { "type": "string", "description": "For type date: this pattern instead of the date-format preference, e.g. \"dd.mm.yyyy\"." },
                "language": { "type": "string", "description": "For type date: month and weekday names in this language code (see fill_sign_date_format) instead of the preference." },
                "author": { "type": "string" },
            }),
            &["doc", "page", "type", "at"],
        )),
        t("fill_sign_date_format", "Date format for Fill & Sign", "Preferences ▸ Date format: the pattern Fill & Sign dates use (default m/d/yyyy). Set it with `format` (Acrobat date codes yyyy yy mmmm mmm mm m dddd ddd dd d, other runs of y, m or d are refused; H h M s t are time letters and need \\ before them; \\ shows the next character as is), or read it. `language` sets the language of month and weekday names (a code from `languages`, or `auto` to follow the app's interface language; English headless). Returns the format, the language, today in them, `unwritable` (characters of today's date that Fill & Sign can't write into a PDF yet, Western European only; fill_sign_add refuses such a date), the ready-made presets and the languages.")
            .with(schema(json!({ "format": { "type": "string", "minLength": 1, "maxLength": 64 }, "language": { "type": "string" } }), &[])),
        t(
            "doc_create",
            "Create a PDF",
            "Create a new, unsaved document and return it like doc_open: `blank` (pages, width, height in points; default 1 US Letter page), `images` (paths of PNG, JPEG, TIFF (every page), GIF or BMP files, one page each at the image's resolution) or `text` (a .txt path, or `text` directly; Western European (WinAnsi) characters only, others are refused). Save it with doc_save and a path.",
        )
        .with(schema(
            json!({
                "from": { "type": "string", "enum": ["blank", "images", "text"] },
                "paths": { "type": "array", "items": { "type": "string" }, "minItems": 1 },
                "dpi": { "type": "number", "minimum": 1, "maximum": 1200, "description": "For images: override the embedded resolution without resampling. 72 gives one point per pixel; omit to use each image's resolution (72 when absent)." },
                "text": { "type": "string" },
                "path": { "type": "string" },
                "pages": { "type": "integer", "minimum": 1, "maximum": 10000 },
                "width": { "type": "number", "minimum": 3 },
                "height": { "type": "number", "minimum": 3 },
                "name": { "type": "string", "description": "Name for the new document (default derived from the source)." },
            }),
            &["from"],
        )),
        t("doc_reduce", "Reduce file size", "Write a smaller copy of the document to `path` with Acrobat's Reduce File Size choices: images above 225 ppi downsampled to 150 ppi and JPEG-compressed (medium quality), thumbnails dropped, identical fonts and images merged, unused objects dropped, compressed object streams. If the copy wouldn't be smaller, nothing is written and `written` is false. The open document is unchanged.")
            .cmd("optimize.reduce")
            .with(schema(json!({ "doc": doc(), "path": { "type": "string" } }), &["doc", "path"])),
        t("doc_initial_view", "Initial view", "Read or change how the document opens (Document Properties ▸ Initial View) and its reading options: navigation (page, bookmarks, pages, attachments, layers), layout (default, single, continuous, two_up, two_up_continuous, two_up_cover, two_up_continuous_cover), magnification (default, actual, fit_page, fit_width, fit_height, fit_visible, or a percentage), page, window options (fit_window, center_window, full_screen, display_title), interface options (hide_menubar, hide_toolbar, hide_window_ui), language and binding (left, right). Only the given ones change; returns the result. Undoable.").with(schema(
            json!({
                "doc": doc(),
                "navigation": { "type": "string", "enum": ["page", "bookmarks", "pages", "attachments", "layers"] },
                "layout": { "type": "string", "enum": ["default", "single", "continuous", "two_up", "two_up_continuous", "two_up_cover", "two_up_continuous_cover"] },
                "magnification": {},
                "page": { "type": "integer", "minimum": 1 },
                "fit_window": { "type": "boolean" },
                "center_window": { "type": "boolean" },
                "full_screen": { "type": "boolean" },
                "display_title": { "type": "boolean" },
                "hide_menubar": { "type": "boolean" },
                "hide_toolbar": { "type": "boolean" },
                "hide_window_ui": { "type": "boolean" },
                "language": { "type": "string" },
                "binding": { "type": "string", "enum": ["left", "right"] },
            }),
            &["doc"],
        )),
        t("doc_audit_space", "Audit space usage", "How many bytes each kind of content takes and its share of the file (PDF Optimizer ▸ Audit space usage): images, content streams, fonts, forms, comments, structure, bookmarks, … and document overhead.")
            .ro()
            .cmd("optimize.advanced")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("text_lines", "List text lines", "The lines of existing text on a page (Edit a PDF ▸ Edit text): number, text, box (top-left-origin points), font and size. Use the number with text_edit.")
            .ro()
            .cmd("edit.edit_text")
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 } }), &["doc", "page"])),
        t("image_hit", "Hit test page artwork", "List image/Form numbers whose placement and all clipping paths contain point [x,y] in top-left-origin points. Bezier curves, even-odd/nonzero fill and holes are respected. Read only.")
            .ro()
            .cmd("edit.edit_text")
            .with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},"point":{"type":"array","items":{"type":"number"},"minItems":2,"maxItems":2}}), &["doc","page","point"])),
        t("page_images", "List page images", "The raster images and grouped Form artwork a page draws (Edit a PDF): number, full placement rect and visible_rect after page/path clipping (top-left-origin points; null if fully hidden), kind (image/form), pixel size ([0,0] for forms) and resource name. Forms include nested graphics and are edited as a whole. Numbers count forms and images together in drawing order, so a page with forms numbers its images differently than before forms were listed: always take numbers from a fresh page_images call.")
            .ro()
            .cmd("edit.edit_text")
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 } }), &["doc", "page"])),
        t("image_edit", "Edit an image", "Change one of a page's images (number from page_images): action move (rect: new full placement box in top-left-origin points, based on page_images.rect), crop (rect: the part to keep, top-left-origin points; the pixels stay in place and page_images.visible_rect shows the result; a later move carries the crop along), rotate (quarters clockwise, default 1), flip_horizontal, flip_vertical, replace (path: an image file, drawn in the same place) or delete. Undoable. The selected image crop transforms with its pixels, isolating shared path scopes while retaining neighboring painting. Text-outline clips return a clear error. Rotation/flip use the visible centre. Form artwork supports move/resize/rotate/flip/delete as a group; replace and image_save only support raster images.")
            .cmd("edit.edit_text")
            .with(schema(
                json!({
                    "doc": doc(),
                    "page": { "type": "integer", "minimum": 1 },
                    "image": { "type": "integer", "minimum": 1 },
                    "action": { "type": "string", "enum": ["move", "crop", "rotate", "flip_horizontal", "flip_vertical", "replace", "delete"] },
                    "rect": { "type": "array", "items": { "type": "number" }, "minItems": 4, "maxItems": 4 },
                    "quarters": { "type": "integer" },
                    "path": { "type": "string" },
                }),
                &["doc", "page", "image", "action"],
            )),
        t("image_save", "Save image as", "Write one of a page's raster images (number from page_images; forms are refused) to path: JPEG images unchanged, others as PNG (the extension is added when missing).")
            .destructive()
            .cmd("edit.edit_text")
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "image": { "type": "integer", "minimum": 1 }, "path": { "type": "string" } }), &["doc", "page", "image", "path"])),
        t("text_font_files", "List installed document fonts", "Discover openly licensed, embeddable TrueType/OpenType files on request. Optional directories (at most 16) restrict the scan; with --root, only that root is scanned by default and supplied folders must stay inside it. The scan skips symlinks, collections and invalid/licence-restricted fonts, with depth, entry and byte budgets. Select a returned path with text_font_import.")
            .ro().cmd("edit.edit_text")
            .with(schema(json!({"directories":{"type":"array","items":{"type":"string"},"maxItems":16}}), &[])),
        t("text_font_import", "Load document font", "Load an explicitly selected, openly licensed full TrueType/OpenType face into a page. The program's family, embedding permissions and licence declaration are validated. Completes source subsets of the same face without replacing existing glyphs. Returns a font resource usable with text_edit.font_resource. Undoable; respects root confinement. No system-interface fallback is exported.")
            .cmd("edit.edit_text")
            .with(schema(json!({"doc":doc(), "page":{"type":"integer","minimum":1}, "path":{"type":"string"}}), &["doc","page","path"])),
        t("text_fonts", "List document fonts", "List validated embedded source faces and approved full preview faces for a page. Full registered resources can be selected by text_edit.font_resource; preview_only faces are not PDF resource names.")
            .ro().cmd("edit.edit_text")
            .with(schema(json!({"doc":doc(), "page":{"type":"integer","minimum":1}}), &["doc","page"])),
        t("text_paragraphs", "List paragraphs", "The paragraphs on a page (adjacent source runs grouped into aligned visual rows, including mixed fonts, sizes and colours): number, text, editor_text (visual rows separated by newlines), rows and source line numbers, box (top-left-origin points), font and size. Use the number with text_edit's paragraph.")
            .ro()
            .cmd("edit.edit_text")
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 } }), &["doc", "page"])),
        t("text_edit_layout", "Measure text draft", "Measure a paragraph draft using the same PDF rewrite as Apply, without changing the document. Returns source_rect, draft_rect (top-left-origin points), overflow, and the embedded sfnt source faces available to the inline editor. Text defaults to the unchanged paragraph. Optional size, width, font_resource and disjoint UTF-8 ranges preview formatting changes.")
            .ro()
            .cmd("edit.edit_text")
            .with(schema(json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "paragraph": { "type": "integer", "minimum": 1 }, "text": { "type": "string" }, "size": { "type": "number" }, "width": { "type": "number" }, "font_resource":{"type":"string"}, "ranges":{"type":"array","maxItems":1024,"items":{"type":"object"}} }), &["doc", "page", "paragraph"])),
        t("text_edit", "Edit text", "Replace the text of one paragraph (paragraph, from text_paragraphs: rewrapped to the paragraph's width with its line spacing) or one line (line, from text_lines) in place, keeping position, size and colour. For a mixed-style region, editor_text supplies visual row breaks; surviving text keeps its source-run styles and inserted text inherits the surrounding run. For a paragraph, also change its formatting: font (helvetica, times, courier) with bold/italic, size (points), color (#rrggbb), align (left, center, right, justify), underline, line_spacing (× size), char_spacing (points) and scale (horizontal, percent), or move it (dx, dy in points; up is +dy) and rewrap it to a new width (points); text may then be omitted. Its own font is reused when it can show every character and no font/weight change is requested; otherwise it uses a standard font matching the source style or, for Japanese and other text no standard font can show (Cyrillic, Greek), real Mincho/Gothic fallback outlines matching the requested/source family and available weight, from a face that has every character (the result shows the font used). Missing Japanese weights use Regular, not synthetic bold; the small web build has only Gothic Regular. Paragraph bold may be set without font to keep the source family. Text that no available font can show is refused. Undoable.")
            .cmd("edit.edit_text")
            .with(schema(
                json!({ "doc": doc(), "page": { "type": "integer", "minimum": 1 }, "line": { "type": "integer", "minimum": 1 },
                    "paragraph": { "type": "integer", "minimum": 1 },
                    "font_resource": { "type": "string", "description": "Explicit source/full page font resource from text_fonts or text_font_import." },
                    "ranges": { "type": "array", "maxItems": 1024, "items": { "type": "object", "properties": {"range":{"type":"array","items":{"type":"integer","minimum":0},"minItems":2,"maxItems":2},"font":{"type":"string","enum":["helvetica","times","courier"]},"bold":{"type":"boolean"},"italic":{"type":"boolean"},"font_resource":{"type":"string"},"size":{"type":"number","exclusiveMinimum":0},"color":{"type":"string"},"underline":{"type":"boolean"},"char_spacing":{"type":"number"},"scale":{"type":"number"}},"required":["range"]}, "description":"Disjoint UTF-8 byte ranges in replacement text; unspecified source formatting survives." },
                    "text": { "type": "string" },
                    "font": { "type": "string", "enum": ["helvetica", "times", "courier"] },
                    "bold": { "type": "boolean" },
                    "italic": { "type": "boolean" },
                    "size": { "type": "number", "exclusiveMinimum": 0 },
                    "color": { "type": "string", "description": "#rrggbb" },
                    "align": { "type": "string", "enum": ["left", "center", "right", "justify"] },
                    "underline": { "type": "boolean" },
                    "line_spacing": { "type": "number", "description": "Multiple of the font size (1.2 is ordinary)." },
                    "dx": { "type": "number", "description": "Paragraph only: move right by this many points (negative: left)." },
                    "dy": { "type": "number", "description": "Paragraph only: move up by this many points (negative: down)." },
                    "width": { "type": "number", "description": "Paragraph only: rewrap to this width in points (dragging the box's edge)." },
                    "char_spacing": { "type": "number", "description": "Points." },
                    "scale": { "type": "number", "description": "Horizontal scale in percent." } }),
                &["doc", "page"],
            )),
        t("doc_revisions", "List revisions", "List the document's saved revisions (oldest first): each incremental update is one. Returns revision number, where it ends in the file and its size, and which signatures sign exactly that revision.")
            .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("doc_open_revision", "Open a revision", "Open saved revision `revision` (1 = the oldest) of a document as a new, unsaved document, to see the file as it was then.")
            .with(schema(json!({ "doc": doc(), "revision": { "type": "integer", "minimum": 1 } }), &["doc", "revision"])),
        t("doc_optimize", "Optimize PDF", "Write an optimized copy to `path` (Acrobat's PDF Optimizer). preset: standard (default; Reduce File Size's choices) or mobile (images to 96 ppi above 144, JPEG high quality); the options below change it. merge_fragments (default true) first puts a page picture drawn as many stacked image strips (as some PDF printers write it) back together into one image, so it can be resampled (merged_images, fragments_merged). color / gray: { downsample, ppi, above_ppi, compression: jpeg|zip|retain, quality 1–100 } (defaults: downsample to 150 ppi above 225, JPEG 60). Images are measured where pages draw them and replaced only when smaller. discard_*: thumbnails (default true), alternate_images (true), tags, print_settings; flate_unencoded (true); remove_invalid_links and remove_unreferenced_dests (true); images and forms the pages list but never draw are always dropped (unused_xobjects). discard: Remove Hidden Information categories (metadata, attachments, comments, form-fields, hidden-text, hidden-layers, bookmarks, links-actions-scripts, private-data). Signed documents are refused. The open document is unchanged.")
            .cmd("optimize.advanced")
            .with(schema(
                json!({
                    "doc": doc(),
                    "path": { "type": "string" },
                    "preset": { "type": "string", "enum": ["standard", "mobile"] },
                    "merge_fragments": { "type": "boolean" },
                    "color": { "type": "object" },
                    "gray": { "type": "object" },
                    "discard_thumbnails": { "type": "boolean" },
                    "discard_alternate_images": { "type": "boolean" },
                    "discard_tags": { "type": "boolean" },
                    "discard_print_settings": { "type": "boolean" },
                    "flate_unencoded": { "type": "boolean" },
                    "remove_invalid_links": { "type": "boolean" },
                    "remove_unreferenced_dests": { "type": "boolean" },
                    "discard": { "type": "array", "items": { "type": "string" } },
                }),
                &["doc", "path"],
            )),
        t("doc_flatten", "Flatten", "Merge comment and/or form field appearances into the page content, so they print and display everywhere but can no longer be edited. Flattening the fields of an XFA form also takes the XFA out (its template, data and scripts), so Adobe's viewers show the baked pages rather than laying the form out again. Undoable until saved.")
            .destructive()
            .with(schema(json!({ "doc": doc(), "comments": { "type": "boolean", "description": "Default true." }, "fields": { "type": "boolean", "description": "Default true." } }), &["doc"])),
        t(
            "xfa_to_static",
            "Convert an XFA form to a static form",
            "Make an XFA form an ordinary PDF form: the XFA packets (template, data, scripts) and PdfCraft's private keys go; the pages as laid out and the fields with their values stay, so every viewer shows the same form and none lays it out again. Its scripts no longer run. Undoable until saved.",
        )
        .destructive()
        .with(schema(json!({ "doc": doc() }), &["doc"])),
        t("edit_undo", "Undo", "Undo the last edit of a document.").cmd("edit.undo").with(schema(json!({ "doc": doc() }), &["doc"])),
        t("edit_redo", "Redo", "Redo the last undone edit of a document.").cmd("edit.redo").with(schema(json!({ "doc": doc() }), &["doc"])),
        t("command_list", "List commands", "Every registered PdfCraft command with its menu, shortcut, whether it is enabled now, and the tool that automates it.")
            .ro()
            .with(schema(json!({ "doc": doc(), "filter":{"type":"string"}, "enabled_only":{"type":"boolean"} }), &[])),
        t("command_run", "Run a command", "Run a registry command through its headless tool. Pass that tool's arguments in params (see command_list). Unknown params are ignored with warnings; path confinement still applies.")
            .destructive().with(schema(json!({"id":{"type":"string"},"params":{"type":"object"}}), &["id"])),
        t("command_batch", "Run several commands", "Run steps in order (at most 1000). Returns completed/failed counts and per-step ok/result/error; stop_on_error defaults to true.")
            .destructive().with(schema(json!({"steps":{"type":"array","items":schema(json!({"id":{"type":"string"},"params":{"type":"object"}}), &["id"])},"stop_on_error":{"type":"boolean"}}), &["steps"])),
        t("doc_inspect", "Inspect open documents", "With doc, return doc_info; otherwise return information for every open document (empty documents array when none are open).")
            .ro().with(schema(json!({"doc":doc()}), &[])),
        t("render_preview", "Render a preview", "Render a PNG bounded by max_side (default 1024), without changing or saving the document. Page defaults to 1; doc may be omitted when exactly one document is open.")
            .ro().with(schema(json!({"doc":doc(),"page":{"type":"integer","minimum":1},"max_side":{"type":"integer","minimum":1,"maximum":4096}}), &[])),
    ]
}

static TOOLS: std::sync::LazyLock<Vec<ToolDef>> = std::sync::LazyLock::new(tools);

pub(crate) fn find(name: &str) -> Option<&'static ToolDef> {
    TOOLS.iter().find(|t| t.name == name)
}

pub(crate) fn tool_for_command(id: &str) -> Option<&'static str> {
    TOOLS.iter().find(|t| t.command == Some(id)).map(|t| t.name)
}

/// Reject unknown and missing arguments (types are checked when each value is read).
pub(crate) fn check_args(def: &ToolDef, args: &Value) -> Result<(), ToolError> {
    let obj = args.as_object().ok_or_else(|| ToolError::InvalidArgs("arguments must be a JSON object".into()))?;
    let props = def.input_schema["properties"].as_object().cloned().unwrap_or_default();
    if let Some(k) = obj.keys().find(|k| !props.contains_key(*k)) {
        let known: Vec<&str> = props.keys().map(String::as_str).collect();
        return Err(ToolError::InvalidArgs(format!("{}: unknown argument {k:?} (expected: {})", def.name, known.join(", "))));
    }
    for r in def.input_schema["required"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if obj.get(r).is_none_or(Value::is_null) {
            return Err(ToolError::InvalidArgs(format!("{}: missing argument {r}", def.name)));
        }
    }
    Ok(())
}
