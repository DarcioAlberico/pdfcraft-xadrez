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
- **Reading** (S8). `board` crops and normalises each diagram, cuts it into 64 squares and classifies them with the piece model. The `pdfcraft-chess` recognizer then makes the reading legal. The geometry is in place (S7b): `board::warp` turns a quad into an upright N × N board (800 px by default), and `board::fit_inner_board` / `board::tighten` trim a frame the contour took along. The whole-board preprocessing the blueprint lists for S7b (deskew, flat-field, hatch suppression, CLAHE) is not done yet.
- **Chess text** (S12–S13):
  - `chess_text` reads a page's text layer (S12, done). `from_page_text(&PageText, lang_hint)` returns `ChessPageText`:
    - `lines`: runs (`Text`, `MoveNumber`, `Move`, `Nag`, `Figurine`), each with a view-space box (`ChessSpan::user_rect` converts to user space);
    - `lang` and `lang_tally`: the piece-letter language, guessed from the page or merged across a document;
    - `quality`: `Good`, `Poor { reason }` or `Missing`, so the engine knows when to fall back to OCR;
    - `unknown_chess_fonts`.

    In catalogued figurine fonts the character code, not the extracted text, says which piece a glyph is, and context decides whether a capital is a piece or a letter. Publisher symbols (`³`) are normalized first, Informator symbol fonts are mapped, and diagram-font glyphs are left to the diagram detector. Tests build `TextGlyph`s directly; no PDF is needed.
  - `glyphs` classifies chess symbols in scanned text.
- **Document** (S14). `doc::ChessDoc` is the recognised document that feeds the PDF-and-text view and every export.

No model weights ship with PdfCraft. `models` loads the user's own ONNX models from `$PDFCRAFT_MODELS` or from beside the executable; on the web, the page passes their bytes. There are two, each with a JSON description: `chess-pieces.onnx` (one 64×64 square → 13 classes) and `chess-glyphs.onnx` (one 32×32 character cell → chess glyphs). The description and the model are checked against each other when they load. `rten` reads ONNX directly, so there is no conversion step. Without the models, detection still works and reading reports them as missing.

To check the loader against real models, point `PDFCRAFT_CHESS_TEST_MODELS` at their folder. A test then loads them and compares their probabilities with reference outputs written beside them (`*.parity.json`, tolerance 1e-4). Without that variable the test is skipped.
