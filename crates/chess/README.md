# pdfcraft-chess

Layer L0, standalone. Holds the chess knowledge that chess-book editing needs, and nothing about PDF: no documents, pages, files or pixels. Because it is standalone it builds for wasm32 and is tested without opening a page.

Planned contents (blueprint `docs/chess/blueprint.md`):

- **Positions and legality** (S2). `placement` holds a piece placement that may be illegal, as read from a book diagram. `fen` parses and formats it. `rules::check_position` reports Legal, IllegalByTurn or FatallyIllegal without needing the side to move. `cozy-chess` (MIT) is used only once a position is known to be legal.
- **Notation** (S2):
  - `san` covers Standard Algebraic Notation in 18 languages and in figurines (♔♕♖♗♘♙).
  - `nag` maps Numeric Annotation Glyphs to printed glyphs and publisher aliases.
  - `catalog` maps the character codes of chess diagram and figurine fonts to pieces.
- **Links** (S2). `lichess::analysis_url` builds the lichess analysis URL for a position. Nothing in this crate touches the network.
- **Recognition post-processing** (S3). `recognize` turns a 64×13 matrix of per-square probabilities into a legal reading: constrained decoding, orientation and side to move, each with the rule that decided it.
- **Drawing** (S4). `draw` turns a placement and a piece set into resolution-free shapes, and `svg` writes them as SVG. These are the single source for replaced diagrams in the PDF and for diagrams in HTML, EPUB, DOCX, RTF and Markdown export.
- **Piece sets** come from the caller, already as shapes. This crate never reads font or image files.

The only external dependency is `cozy-chess` (MIT), with the x86-only `pext` feature off.
