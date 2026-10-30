# Chess piece sets

Every folder here is one piece set that ships with PdfCraft: twelve SVG files named `wP.svg` … `wK.svg` and `bP.svg` … `bK.svg`, compiled in by `crates/chessvision/src/pieces/bundled.rs`. `ATTRIBUTION.toml` has one entry per file.

## Sets by other authors

`chessnut`, `rhosgfx`, `kiwen-suwi`, `firi`, `fantasy`, `spatial`, `celtic` and `shapes` are openly licensed sets by their own authors, as lichess publishes them. Each folder holds its licence (and, where the author asks for one, a copyright notice) beside the files.

## Sets drawn for PdfCraft

These twelve are contributor-original, licensed under the project's licence (`MIT OR Apache-2.0`):

| Folder | Name | Look |
|---|---|---|
| `classic` | Classic | Turned Staunton-style silhouettes, white with a black outline, black with white detail lines |
| `tokens` | Tokens | Round counters with a rim, the classic silhouette inside |
| `pixelart` | Pixel Art | 16 × 16 pixel drawings in three tones |
| `bauhaus` | Bauhaus | Circles, squares and triangles, one primary colour on each piece |
| `facets` | Facets | Angular silhouettes cut into flat-shaded triangles (low poly) |
| `glass` | Stained Glass | The same silhouettes as glass panes in dark leading: pale panes for White, deep ones for Black |
| `moves` | Moves | A tile with a 5 × 5 board: the piece in the middle and the squares it reaches (a teaching set; Black's pawn moves down, as White sees the board) |
| `chubby` | Chubby | Round pieces with faces, for young players |
| `deco` | Art Deco | Tall stepped forms, ivory and ebony with gold lines |
| `gothic` | Gothic | Pointed arches, spires and lancet windows |
| `wood` | Wood | Turned pieces with beads and rings, maple and walnut |
| `sketch` | Sketch | The classic silhouettes drawn twice by a wavering pencil line |

They were drawn from scratch for PdfCraft on 2026-10-30 by Claude, the AI coding agent, at the project owner's request: circles, polygons and Bézier curves written as SVG, and pixel drawings typed out square by square. None is traced from, or derived from, another set, font or image.

### Drawing rules

Diagrams are flat, so a piece that should draw the same everywhere (the boards, the PDF, every export) keeps to what the SVG reader keeps (`crates/chessvision/src/pieces/svg.rs`):

- a `viewBox` of `0 0 100 100`, the piece standing on a base near the bottom;
- solid fills and strokes only: a gradient is drawn in its first stop's colour, and opacity, clip paths, masks, filters and text are not kept;
- strokes are drawn with round joins.

White's pieces are the light ones and Black's the dark ones: the tests check that each set's white pieces are lighter than its black pieces and that no two of its pieces are drawn alike.
