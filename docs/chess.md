# Chess books

**Status:** current to 2026-10-18 (blueprint steps S1–S18; see [`chess/blueprint.md`](chess/blueprint.md)).
**Revision history:** 2026-10-18 — first version, after S18.

PdfCraft can read a chess book in PDF. It finds the diagrams, reads each one as a position (FEN), and reads the moves in the text, including figurines and annotation signs. It can redraw a diagram, put a lichess link under it, and export the book as HTML, EPUB, Word, RTF, Markdown or PGN. Everything runs on your machine; nothing is sent anywhere.

## What you need

None of these files ship with PdfCraft (fork decisions D2 and D4). Without them, the rest still works.

| File | What it does | Without it |
|---|---|---|
| `chess-pieces.onnx` + `.json` | reads drawn and scanned diagrams | only diagrams set in a chess diagram font are read; the others are found and marked |
| `chess-glyphs.onnx` + `.json` | chess OCR: reads moves, figurines and signs from page images | pages without a usable text layer are listed as needing OCR |
| `text-detection.rten`, `text-recognition.rten` (ocrs) | reads the prose around the notation | the glyph reader reads the prose too, less well |
| a chess diagram font (Chess Merida) | draws redrawn diagrams with real pieces | plain geometric pieces |

PdfCraft looks for the models in the folder named by `PDFCRAFT_MODELS`, then beside the executable. For the pieces it uses the font named by `PDFCRAFT_CHESS_PIECES`, then an installed Chess Merida. A diagram drawn with Merida or cburnett pieces carries those pieces into what you export. That is fine for personal use, but not for publishing.

## In the app

Open the **Chess diagrams** panel (`chess.panel`), or run **Chess ▸ Scan** (`chess.scan`).

- **Scan this page** or **Scan all pages** find the diagrams. Pages are scanned in batches in the background, and each batch shows as soon as it is done.
  - Boxes on the page show each diagram's state: read, needs review, not read, or failed.
  - Any edit makes the scan stale. The boxes leave the page until you scan again.
- **Pick a diagram**, in the list or on the page, to open the **editor**.
  - The editor shows the position on a board. Click a square to put the chosen piece there, and click it again to empty it.
  - You can also set the side to move and the castling rights, flip the board, or go back to the reading.
  - The editor says at once whether the position is legal.
  - Squares the model was unsure of are outlined. Each comes with its second reading, which you can take in one click.
- From the editor:
  - **Replace on the page** draws the corrected position over the diagram, as one undo step.
  - **Add lichess link** puts a link to the corrected position under the diagram.
  - **Copy FEN** and **Open in lichess** work on the position as edited.
- The **review queue** holds the diagrams that are not read, could not be read, or did not pass the reading gate. Repaired readings never pass the gate.
  - **Only diagrams to review** filters the list.
  - **Previous / Next** walk the queue in reading order.
- The **Text** tab shows the page beside it read as a chess document: headings, paragraphs and moves.
  - Moves are coloured by what replaying them found: valid, illegal, not reached, or unchecked.
  - Click a move or a paragraph to see where it is on the page, or click a diagram to edit it.
  - **Read this page with chess OCR** reads the page image instead of its text layer.
- **Export…** writes the whole book as HTML, EPUB, Word, RTF, Markdown or PGN. Markdown's diagram images go in a folder beside the file.

## Headless: tools and CLI

Every feature is also a tool (`pdfcraft-cli tools`; the MCP server is opt-in):

| Tool | Does |
|---|---|
| `chess_status` | which models and piece set are available, and why not |
| `chess_detect`, `chess_read` | find the diagrams; read them (FEN, legality, side to move, weakest square, uncertain squares, repairs, lichess URL) |
| `chess_document` | the book as headings, paragraphs of moves and diagrams, with the games replayed into variation trees; JSON and PGN |
| `chess_replace_diagram`, `content_draw` | redraw a diagram (one undo step); the generic checked-operators edit under it |
| `chess_add_lichess_links` | a label and link under every legal, accepted diagram |
| `chess_export_html`, `_epub`, `_docx`, `_rtf`, `_markdown`, `_pgn` | the exports |

`pdfcraft-cli chess scan FILE` prints the diagrams found. All paths are confined to `--root`.

The pages to OCR are chosen with `ocr`: `auto` (the default) reads only pages whose text layer is poor or missing, `off` reads none, and `all` reads every page.

## How well it reads

All figures below are on the user's own models. Each has its command and the machine it ran on in the blueprint's log (§8).

- **Finding diagrams on a page image:** recall 0.9826 (113 of 115), precision 1.0, at 220 dpi on the prototype's field set.
- **Reading diagrams with the piece model:** 555 of 569 exact (0.9754) on the prototype's test split. The target is 0.99.
- **Chess OCR, fused with ocrs:** CER 0.88% and 98.0% of moves exact on the Caïssa benchmark (744 lines). These numbers are in-sample: the Dvoretsky lines are in the model's training data.
  - A scanned line of ten Helvetica moves is read 10 of 10.
  - Outline figurines in fonts the model never saw are still weak, at about 74% of moves for DejaVu Sans.
- Diagrams set in a chess diagram font are read exactly, from the text.

**Speed and a real book** (2026-10-18, `pdfcraft-cli run` in release, the user's models, Windows 11 on an AMD desktop; *Dvoretsky's Endgame Manual* (2025), pages 51–60, one process per run, so nothing cached):

| Step | Time | Result |
|---|---|---|
| Find the diagrams | 0.31 s/page | 19 diagrams |
| Find and read them | 0.53 s/page | 19 of 19 accepted |
| The recognised document (text layer) | 0.86 s/page | 420 moves, 12 games, 61 valid (7 before column order) |
| The recognised document with chess OCR | 1.8 s/page | |

That PDF's text layer is an old OCR layer (words like `Taf7asch` for Tarrasch), and its pages are set in two columns. Lines are now cut at the gutter and read column by column, and a diagram's printed coordinates are left out: valid moves went from 7 to 61. Most of the rest are "not reached": the text often goes on with a game whose first moves are on an earlier page ("5 Kg6" under a diagram of the starting position), and a line stops at its first move that cannot be played.

## Limits

- **Moves and text**
  - A game that goes on from an earlier page, or past a move that cannot be played, is not followed: its later moves are "not reached" (see above).
  - Three or more columns are read as two at most.
  - Umlauts in prose come through only where the glyph reader has them, because ocrs reads ASCII only.
- **Links:** the lichess address is plain text in Word and RTF; it is a working link in PDF, HTML, EPUB and Markdown.
- **Readings and corrections**
  - A reading the decoder repaired is never linked or accepted until a person confirms it.
  - The book's cipher (figurines a font maps to other letters) is learned per run, not kept with the document.
- **Speed:** reading a page with OCR, or exporting a whole book, runs off the interface thread (the panel shows it is working) but cannot be cancelled once started.
