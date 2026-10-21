# pdfcraft-chessvision

Layer L4. It finds chess diagrams on PDF pages, reads them into positions and reads chess text (figurines, moves, NAGs). Its results are values from `pdfcraft-chess`.

It depends on no other L4 crate. The engine (L6) adapts `edit`'s page images and paths into this crate's own input types, and turns the results into edits: replaced diagrams, lichess links and a text layer.

Planned contents (blueprint `docs/chess/blueprint.md`):

- **Detection** (S7a–S7c). Candidates come from four sources:
  - embedded images;
  - contours on the rendered page, using `rten-imageproc` (no OpenCV);
  - text in chess diagram fonts;
  - vector-drawn grids.

  `merge` joins candidates from different sources into one diagram each. `order` puts them in reading order, the single ordering shared by the screen, export, links and tools.

  Embedded images are done (S7a): `detect::embedded::candidates_from_images` takes the page's `ImageBox`es (box in user space, pixel size, optional gray raster as a `raster::GrayImage`) and drops background scans, joins touching tiles, filters by pixel size, aspect and size on the page, and trims frame and caption off with `trim_to_grid` / `trim_to_frame`. Its thresholds, ported from the Python prototype's measurements, are in `detect::embedded::consts`.

  Page contours are done (S7b): `detect::contour::candidates(page, geometry, dpi)` takes the whole displayed page rendered to gray at a known DPI. It thresholds the page adaptively (block 41, C 8) in three binarisations, simplifies each contour to a quad, and scores it as geometry × (0.55 + 0.45 × texture), where texture is checker contrast and grid regularity on a 320 px warp. A quad with no checker contrast (a photo, a frame, a table) is dropped before it competes. A second search runs on the page at half size, and a square rescue handles a board joined to its caption. Candidates carry the quad's corners in user space (`DiagramCandidate::corners`) for the reader to warp from. The detector has no OpenCV: `rten-imageproc` finds contours and minimum-area rectangles, and the blur, threshold, morphology, resizing and warp are written in this crate. Pages above 12 MP are shrunk first. The constants are in `detect::contour::consts`. On the prototype's 68-page field set, the contour path alone has recall 0.9826 at 220 DPI and 0.9913 at 150 and 300 DPI, with precision 1.0 at all three (`examples/contour_bench.rs`, which needs the books locally).
- **Coordinates.** Boxes travel in PDF user space (points), with the DPI they were measured at. `coords` is the one place that converts them: `PageGeometry` (crop box, `/Rotate`, `/UserUnit`) maps user space to view space (y down, after `/Rotate`, as rendered pixels and `TextGlyph.rect`) and back, and pixel boxes take their DPI explicitly.
- **Reading** (S8, done). `board::read(page, geometry, dpi, candidates, model)` returns one `BoardReading` per candidate, in the candidates' order:
  - a candidate with a placement (a diagram font) is read exactly and skips the model;
  - otherwise `board::board_image` cuts the board: from its corners (warp to 800 px, then `tighten`) or from its box (cut, stretched to 800 px), through `coords`, so `/Rotate` and turned boards work;
  - `board::squares` cuts it into 64 squares of 64 × 64 gray (100 px cells edge to edge, area-averaged, rounded to 8 bits, ÷ 255), and each board also sends its squares turned 180°;
  - up to 8 boards (1024 squares) go to the model in one call;
  - the `pdfcraft-chess` recognizer decodes both readings under the rules, picks the orientation and the side to move, and gates the reading (legal, weakest square ≥ 0.80).

  The result carries the placement, FEN, legality, side, orientation verdict, minimum confidence, uncertain squares, repairs, the gate, the DPI and the source (model or font). An unreadable board (no model, bad geometry, off the page, a failing model) is a `Unreadable` value, not an error. The preprocessing is the prototype's (`split_board_into_cells`, `preprocess_cell_to_tensor`, `predict_with_orientation`), since the model was trained on it. `SquareClassifier` lets tests stand in for the model. On the prototype's test split (569 boards), `examples/read_bench.rs` reads 555 exactly (0.9754), board for board the same as the prototype with the same ONNX model; the blueprint's 0.99 target is not met by the model on this split. The whole-board preprocessing the blueprint lists for S7b (deskew, flat-field, hatch suppression, CLAHE) is not done yet.
- **Chess text** (S12–S13):
  - `chess_text` reads a page's text layer (S12, done). `from_page_text(&PageText, lang_hint)` returns `ChessPageText`:
    - `lines`: runs (`Text`, `MoveNumber`, `Move`, `Nag`, `Figurine`), each with a view-space box (`ChessSpan::user_rect` converts to user space);
    - `lang` and `lang_tally`: the piece-letter language, guessed from the page or merged across a document;
    - `quality`: `Good`, `Poor { reason }` or `Missing`, so the engine knows when to fall back to OCR;
    - `unknown_chess_fonts`.

    In catalogued figurine fonts the character code, not the extracted text, says which piece a glyph is, and context decides whether a capital is a piece or a letter. Publisher symbols (`³`) are normalized first, Informator symbol fonts are mapped, and diagram-font glyphs are left to the diagram detector. Tests build `TextGlyph`s directly; no PDF is needed.
  - `glyphs` reads scanned chess text (S13b, the reader; the engine's `recognize_chess` comes later). `glyphs::read(image, region, scale, classifier, options)` segments a gray raster into lines of words and classifies each glyph with the user's 314-class glyph model (any `GlyphClassifier`; tests use stand-ins):
    - Otsu (or a local threshold), 8-connected components, and the text scale (`text_scale(page, diagrams)` for a page; a crop measures its own);
    - character boxes, then joins: dots and accents back on their stems (with an italic correction), the halves of `:` `;` `=`, and the two parts of `±` `∓` `⩲` `⩱`;
    - two arbiters where the classifier decides the segmentation: a wide box is cut at its ink valley when the halves read better, and overlapping neighbours merge when the union reads better;
    - 32 × 32 bilinear cells, then corrections: `=`/`:` by shape, a square dot read as a dash, `0`/`o` in numbers and castling, the case of `cosuvwxz` by height, and a split move number (`1 9...`) joined on notation lines.

    Each glyph keeps its top three readings and the margin between the first two, for the cipher and legality repair (S13c) and the review queue. `glyphs::fuse::fuse_line` fuses the glyph reader with a general OCR's words (passed in as plain `OcrWord`s; this crate does not depend on `pdfcraft-ocr`): word groups that overlap in columns take the glyph reading when they are notation (a figurine or symbol, a move, a move number, only marks) and the OCR's otherwise. `fuse::to_chess_text` reads the result through `chess_text::from_page_text`, so OCR and text layers give the same `ChessPageText`. Every input-derived size is capped (`MAX_GLYPH_PIXELS`, `MAX_COMPONENTS`).

    `examples/ocr_bench.rs` scores it on the Caïssa ground truth (744 line crops, which stay outside the repository) with Caïssa's instrument re-derived in Rust: fused with `ocrs`, CER 0.89% and 98.0% of moves (640/653); the glyph reader alone, CER 1.25% and 97.9%. The Dvoretsky lines (719 of 744) are in the classifier's training data, so these are in-sample numbers. On the same text typeset in fonts the classifier never saw, figurines in solid fonts read perfectly (Times + Segoe UI Symbol: 99.5% of moves), but outlined DejaVu figurines still fail about 29% of the time (73.5% of moves).
- **Document** (S14a here; S14b in the engine: `Session::chess_document` gathers the inputs from a document's pages, and the `chess_document` tool returns or writes the result). `doc::ChessDoc` is the recognised document that feeds the PDF-and-text view and every export:
  - pages of blocks (`Heading`, `Paragraph`, `Diagram`) of inlines (`Text`, `Move`, `Nag`, `Link`), each with a `source_rect` in PDF user space;
  - built page by page with `doc::DocBuilder` from plain inputs (`PageInput`): the page's `ChessPageText` (text layer or OCR), its `BoardReading`s in the one reading order, its `PageGeometry`, optional `BlockHint`s (headings and paragraphs, which the engine takes from `edit::reading_blocks`) and the S13c repairs as `TextReview`s. Diagrams go among the lines by position (below them in their column, or in a column to their right) and are never reordered, so diagram n is the n-th `Block::Diagram`;
  - a diagram block carries its reading (FEN, legal, accepted, weakest square, repaired, side assumed), the lichess link (`ReadPosition::lichess_url`: legal and not repaired) and its review flags;
  - `doc::games` replays the moves with `cozy-chess` into variation trees: from the diagram before them, or from the initial position at `1.`. Brackets are variations, and a numbered move that points back into its line starts an alternative. A numbered move that is exactly the main line's next move goes back to the main line after a prose digression instead of becoming an alternative inside it. An illegal, ambiguous or out-of-sequence move is flagged and stops its line; nothing is dropped (unplayed moves stay in the text with their status and in the game's comments). A diagram that did not pass the gate still seeds its game, which is marked untrusted;
  - `doc::pgn` writes the games (roster, `SetUp`/`FEN`, NAGs, comments, variations) and reads them back; `parse(write(game)) == game` is property-tested on random legal games;
  - everything is serde JSON (`ChessDoc::to_json`, `from_json`), and everything is bounded (moves, variation depth, comment length, games). `DocBuilder::add_read_page` judges the moves of pages read before again (a document's JSON, or pages after a correction); `examples/walk_bench.rs` uses it to measure the game walk alone on documents read earlier.
  - `doc::html::write` (S15a) makes one static HTML file: headings, paragraphs with moves (Unicode figurines, letters of any language, or as printed), NAG glyphs, and each diagram redrawn as inline SVG (`pdfcraft_chess::draw`) or as FEN, with its lichess link. No script; only http(s) links; readable back through `data-fen`, `data-san`, `data-nag` and the `unread`/`review` classes.

No model weights ship with PdfCraft. `models` loads the user's own ONNX models from the folder chosen in the app, then `$PDFCRAFT_MODELS`, then beside the executable (`ChessModels::locate`); on the web the user picks the files and the page passes their bytes (`ChessModels::from_files`, at most 64 MB a file). There are two, each with a JSON description: `chess-pieces.onnx` (one 64×64 square → 13 classes) and `chess-glyphs.onnx` (one 32×32 character cell → chess glyphs). The description and the model are checked against each other when they load. `rten` reads ONNX directly, so there is no conversion step. Without the models, detection still works and reading reports them as missing.

To check the loader against real models, point `PDFCRAFT_CHESS_TEST_MODELS` at their folder. A test then loads them and compares their probabilities with reference outputs written beside them (`*.parity.json`, tolerance 1e-4). Without that variable the test is skipped.
