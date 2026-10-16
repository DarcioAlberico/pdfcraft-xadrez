# Proveniência e relicenciamento (D1)

**Data:** 2026-10-15 · **Titular:** DarcioAlberico (GitHub)

## Declaração
Eu, titular dos repositórios abaixo, licencio sob **`MIT OR Apache-2.0`** os módulos marcados como **próprio** nesta lista. O objetivo é permitir que suas ideias, dados, constantes e algoritmos sejam reimplementados em Rust no `DarcioAlberico/pdfcraft-xadrez`.

Isto **não** relicencia:
- dependências como python-chess (GPL-3.0+), PyMuPDF (AGPL), PyQt6 (GPL);
- código de terceiros (§3).

Repositórios cobertos, todos com commits só de DarcioAlberico:
- `DarcioAlberico/ChessVisionOFF_Puro` (371 commits)
- `DarcioAlberico/Suite_de_Edicao_de_Xadrez` (139)
- `DarcioAlberico/Editor_Diagramas_de_Xadrez`
- `DarcioAlberico/PyBoxEditor_Tkinter` (privado)
- `PGN_Live_Editor`: local, sem remote. 13 commits de DarcioAlberico e 5 de "PGN Live Editor <dev@localhost>". O titular confirmou em 2026-10-15 que essa identidade também é dele.

## 1. ChessVisionOFF_Puro (`src/chess_diagram_ocr`)

| arquivo | situação | python-chess (substituir na reimplementação) |
|---|---|---|
| config.py, decode.py, orientation.py, preprocess.py, board_detection.py, detection/embedded.py, detection/hybrid.py, model.py, onnx_export.py, gallery.py | próprio | — |
| fen_utils.py | próprio | Board, Status, STATUS_* → `chess::rules::check_position` sobre `cozy-chess` |
| semantics.py | próprio | Board, Color → idem |
| pdf_text.py | próprio | Color, WHITE/BLACK (só constantes) |
| text/notacao.py | próprio | Board, Move, erros de SAN → `chess::san` |
| pdf_substituicao.py | próprio, absorvido do Editor_Diagramas (seu) | WHITE/BLACK |
| desenho_de_diagrama.py | próprio, absorvido do Editor_Diagramas (seu) | `chess.svg.board()` → `chess::draw` + `svg` (**não** reproduzir o SVG do python-chess) |
| text/modelo.py e o pacote `text/` | próprio, portado do PyBoxEditor_Tkinter (seu) | — |

## 2. Suite_de_Edicao_de_Xadrez (`src/caissa`)

| arquivo | situação | python-chess |
|---|---|---|
| core/chess/notation_tables.py, ocr/notation/cipher.py, ocr/notation/book_cipher.py, export/fidelity.py | próprio | — |
| ocr/metrics.py (métrica CER/lances, portada em `crates/chessvision/examples/ocr_bench.rs`) | próprio (único autor no histórico) | — |
| notation/legality_repair.py | próprio (cita PGN_Live_Editor como ideia) | Board, Move → `cozy-chess` |
| ocr/notation/validate.py (`_best_assignment` e a guarda de região, portados em `chess::repair` e `chessvision::cipher`) | próprio (único autor no histórico) | Board, STARTING_FEN → `chess::repair::GameContext` sobre `cozy-chess` |
| notation/nag_table.py | próprio (portado do PGN_Live_Editor, seu) | — |
| notation/annotations.py | próprio (portado do PGN_Live_Editor, seu) | square helpers |
| vision/detect/font_catalog.py | próprio + PDFimport **com permissão** (§3) | — |
| vision/detect/vector_detect.py | próprio + PDFimport **com permissão** (§3) | — |
| ingest/pdf/textlayer.py | próprio + PDFimport **com permissão** (§3) | — |

## 3. Terceiros
- **PDFimport** (`C:\Python-Chess2\PDFimport`): plugin do Sigil de autoria de Wagner Escorcio (`plugin.xml`). O titular declarou em 2026-10-15 que **tem permissão do autor** para usá-lo.
  - Com isso, os trechos derivados dele nos três arquivos mistos podem ser reimplementados no projeto.
  - **Para fechar a cadeia:** guardar uma cópia da permissão por escrito em `docs/chess/permissions/` (no repositório privado) e confirmar que ela cobre a distribuição sob `MIT OR Apache-2.0`. Se cobrir só o uso, os trechos continuam utilizáveis no repositório privado e são re-derivados da ISO 32000 antes de qualquer publicação.
  - **A permissão cobre só o código do próprio plugin.** A cópia de PyMuPDF que vem em `lib/pymupdf` é da Artifex, sob AGPL, e continua excluída.
- **python-chess** (GPL-3.0+): dependência, não código copiado. Toda lógica que se apoia nele é reescrita sobre `cozy-chess` (MIT).
- **Chess_diagram_to_FEN** (tsoj): não usado.
- **Pesos** (`*.pt`): não distribuídos (D2). **Peças cburnett e fonte Merida**: carregadas em tempo de execução da máquina do usuário, nunca commitadas (D4).
- **SimbolosDeXadrez.ttf**: origem desconhecida, não usar.

## Pendências do titular
- [x] "PGN Live Editor <dev@localhost>" é o titular (confirmado em 2026-10-15).
- [x] Há permissão de Wagner Escorcio para o PDFimport (declarado em 2026-10-15).
- [ ] Guardar uma cópia da permissão em `docs/chess/permissions/` e anotar se ela cobre a distribuição sob MIT/Apache. Isso só bloqueia a **publicação**, não o desenvolvimento.
