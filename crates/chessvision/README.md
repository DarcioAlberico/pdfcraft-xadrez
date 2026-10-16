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
- **Coordinates.** Boxes travel in PDF user space (points), with the DPI they were measured at. `coords` is the one place that converts them: `PageGeometry` (crop box, `/Rotate`, `/UserUnit`) maps user space to view space (y down, after `/Rotate`, as rendered pixels and `TextGlyph.rect`) and back, and pixel boxes take their DPI explicitly.
- **Reading** (S8). `board` crops and normalises each diagram, cuts it into 64 squares and classifies them with the piece model. The `pdfcraft-chess` recognizer then makes the reading legal.
- **Chess text** (S12–S13):
  - `chess_text` reads figurine fonts in a page's text layer;
  - `glyphs` classifies chess symbols in scanned text.
- **Document** (S14). `doc::ChessDoc` is the recognised document that feeds the PDF-and-text view and every export.

No model weights ship with PdfCraft. `models` loads the user's own `.rten` files from `$PDFCRAFT_MODELS`, or from bytes on the web. Without them, detection still works and reading reports the models as missing.
