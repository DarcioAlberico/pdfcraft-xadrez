# Blueprint — Edição de PDF de xadrez no PdfCraft

**Objetivo.** Transformar o PdfCraft num editor de livros de xadrez em PDF. Ele deve:
- localizar e ler diagramas (FEN), substituí-los por um diagrama redesenhado e pôr um link do lichess abaixo de cada um;
- reconhecer o texto de xadrez, com figurinas (♔♕♖♗♘♙) e sinais do Informator (± ∓ ⩲ ∞ □ △ …);
- converter o PDF em HTML, EPUB, RTF, DOCX, Markdown e PGN;
- mostrar a página e o texto reconhecido lado a lado ou em duas abas.

**Status:** v4 · 2026-10-19 · **S1–S18 concluídos** (§8) · S0b em revisão (#55) · decisões D1–D4 tomadas (§1)
**Cópia canônica:** `C:\Python-Chess2\Chess PDF-Craft\plan\chess\pdfcraft-chess-editing.md`. O `plan/` é gitignored, então worktrees e agentes **não** o veem. O S1 commita uma cópia em `docs/chess/blueprint.md`; até lá, citar sempre o caminho absoluto.
**Fontes reaproveitadas:** `C:\Python-Chess2\ChessVisionOFF_Puro` ("tronco") e `C:\Python-Chess2\Suite_de_Edicao_de_Xadrez` ("Caïssa").
**Repositório:** `https://github.com/DarcioAlberico/pdfcraft-xadrez` (**privado**, independente, sem ser fork do GitHub, com o histórico do PdfCraft).
- `origin` = esse repositório.
- `upstream` = `storytold/pdfcraft`, com push **desativado** de propósito.
- GitHub Actions **desligado** no repositório.

---

## 0. Leia antes de qualquer passo

### 0.1 O que o PdfCraft exige (`CLAUDE.md` / `AGENTS.md` — inegociável)

| Regra | Consequência para o xadrez |
|---|---|
| **Só Rust** no produto e no build | Nenhum Python roda no app. Modelos PyTorch entram como `.rten` (ONNX → `rten-convert`, versão casada com o `rten 0.26` do lockfile) e rodam no `rten`, que já é dependência via `crates/ocr`. O treino fica fora do repo. |
| **Nada de código GPL/AGPL** | Ficam de fora: python-chess (GPL-3), PyMuPDF (AGPL), PyQt6 (GPL), `shakmaty`/`pgn-reader` (GPL-3). Regras via `cozy-chess` (MIT, já aceito pelo `deny.toml:10`). |
| **Assets só com licença provável** + `ATTRIBUTION.toml` com SHA-256 | Peças cburnett (GPL; a opção "BSD" da Commons não nomeia variante, logo não é provável), Merida e ChessBase **não entram**. Pesos sem licença ou treinados em recortes de livros com copyright **não são distribuídos** (D2). |
| **Fontes só no `storytold/craft-fonts`** | Fonte de figurinas e sinais (DejaVu Sans, Bitstream-Vera; Noto Sans Math, OFL, para ⩲ U+2A72 e afins) é **dependência externa**: depende do dono do craft-fonts. O código funciona sem ela. |
| **Nunca entrar em pânico** | Sem `unwrap`/indexação em dado do PDF ou do modelo. A matriz do `rten` é validada (forma, NaN). Toda correção de crash ganha teste sintético visto falhando. |
| **Camadas** (`xtask/src/layers.rs`: `TABLE` :31, `SIDEWAYS` :98) | Crate só depende de camada **inferior**. Mesma camada só via `SIDEWAYS`, cada entrada com aval do dono. **Não** acrescentar `SIDEWAYS`: a orquestração vai para o engine (L6). Dependências externas não entram na checagem de camadas. |
| **WASM** | Todo crate L0–L7 compila em `wasm32-unknown-unknown`. No wasm, `Models::find` devolve `None` (usa env, `current_exe`, `CARGO_MANIFEST_DIR`): modelos de xadrez carregam **por bytes**, escolhidos pelo usuário. |
| **Tudo headless** | Cada funcionalidade tem ferramenta em `crates/automation`, teste ponta a ponta e entrada em `parity/acrobat-features.toml`, que é o **único** arquivo que `cargo xtask parity` valida (`xtask/src/parity.rs:162`). |
| **Sem rede de saída** · **sem JS escrito à mão** | Link do lichess é só anotação URI. "Abrir no lichess" usa `PdfCraftApp::open_url` (`ui-egui/src/lib.rs:1939`), por ação do usuário. HTML/EPUB exportados **sem JS**: nada de tabuleiro interativo. |

### 0.2 Mapa do que já existe no PdfCraft (conferido)

| Precisa de | Já existe em |
|---|---|
| Imagens da página | `crates/edit/src/images.rs`: `PageImage` (rect em **espaço do usuário**, :16), `page_images` :213, `ImageChange` :372, `change_image` :448 · `ImageEdit::Replace{name, bytes}` (bytes de **arquivo de imagem**) e `Delete` (`engine/src/lib.rs:74-85`) |
| Desenho novo | `edit::added::add_content` (stream `/PCMark /Added`) e `edit::stamp` (`edit/src/lib.rs:385`). **Não há `Edit` de conteúdo vetorial arbitrário.** As `Add*` (`engine/src/lib.rs:1287-1666`) são marcador, carimbo, campo, texto OCR, cabeçalho, marca-d'água, fundo, texto, imagem e links. |
| Links URI | `crates/annot/src/links.rs` (`add` :220, `add_many` :403) · `Edit::AddLink` :1643 (espaço do usuário) / `AddLinks` :1666 |
| Texto avulso | `Edit::AddText` (**espaço de exibição**, :1606), Helvetica/WinAnsi: "↗" e figurinas não existem ali |
| Um passo de desfazer | `Edit::Batch{label, edits}` :1699 |
| Texto com posição | `crates/render/src/text.rs`: `TextGlyph{text, rect, quad, direction}` :43 (rect em **espaço de visão, y para baixo**; `text` já é Unicode via ToUnicode do hayro; **sem fonte nem código bruto**), `layout()` :1302 · `edit::reading_blocks` (`edit/src/text.rs:1889`) |
| Objetos da página | `edit/src/objects.rs` `editable_objects` :54 (Added, Text, Image; **sem paths**). O interpretador de `crates/redact/src/interp.rs` é um reescritor de redação de 884 linhas, não um walker reutilizável. |
| OCR | `crates/ocr` (`ocrs 0.13` + `rten 0.26`; `ALPHABET` ASCII + € :24; `Word{text, rect}` :90, **sem confiança por caractere**; `text_layer` Helvetica/WinAnsi :217-240) · `engine/src/ocr.rs` (`OcrJob` em thread; inline no wasm, `ocr_ui.rs:184-191`) |
| Visão/geom. sem OpenCV | `rten-imageproc 0.26` (Cargo.lock :5366): `find_contours`, `simplify_polygon`, `min_area_rect`; já roda no wasm |
| Modelos | `cargo xtask models` → `assets/models/` · `Models::find` (`ocr/src/lib.rs:59-73`) · `[[fetched]] kind = "model"` (`ATTRIBUTION.toml:3299`) |
| Exportação | `crates/export`: `Block` é **struct** (:17); `Page{blocks, images}` deriva `Default`. Funções `html` :360, `docx` :632, `rtf` :770, `pptx`, `Zip`. `engine/src/compare.rs`: `OfficeFormat` :126, `export_pages` :157, `export_office` :211. **Sem EPUB nem Markdown.** |
| Vista dividida | `ui-egui/src/compare_ui.rs` `side_by_side` :290 / `compare_area` :300 |
| UI, comandos, i18n | `RightPanel` (`ui-egui/src/lib.rs:191`), `docking.rs` · registro `engine/src/commands.rs` · despacho `ui-egui/src/commands.rs:103` · grupos `engine/src/catalog.rs` · i18n `ui-egui/src/i18n/{en,pt-br,ja}.tsv`. O teste `japanese_covers_commands_and_catalogue` (`i18n/mod.rs:810`) **exige ja.tsv** para todo comando, grupo e item. |
| Ferramentas | `automation/src/tools.rs` (`ToolDef`, `t(..).cmd(..).with(schema(..))`) · `Automation::call` (`lib.rs:185`) · `tests/automation.rs` |

### 0.3 Reaproveitamento dos projetos Python

**Regra (D1):** reaproveitam-se **dados, constantes, algoritmos e medições**, reescritos em Rust a partir da especificação.
- Nenhum arquivo é traduzido linha a linha.
- Onde o Python usa python-chess, re-derivar das regras do jogo e dos padrões FEN/PGN/SAN, sem copiar a estrutura.
- Afetados: `semantics.py:31`, `pdf_substituicao.py`, `text/notacao.py`, `fen_utils.py`.

| Origem | O que leva | Dificuldade |
|---|---|---|
| Caïssa `core/chess/notation_tables.py` | letras de peça em 18 idiomas, figurinas | fácil (dados) |
| Caïssa `notation/nag_table.py` + `BOOK_SYMBOL_ALIASES` | NAG ↔ glifo ↔ aliases de editora (antes do NFKC) | fácil (dados) |
| Caïssa `vision/detect/font_catalog.py` | layouts de fontes de diagrama + lista de fontes de figurina. Os trechos do PDFimport são usados com permissão do autor (`provenance.md` §3). Guardar a permissão antes de publicar. | fácil (dados) |
| Tronco `decode.py` | decodificação com restrições, `MAX_DECODE_CHANGES = 6` | fácil |
| Tronco `orientation.py` | cascata de 5 regras, margem 0,20 | fácil |
| Tronco `semantics.py`, `pdf_text.py` | lado a jogar; padrões de legenda pt/en; símbolos ◻/◼ | médio (re-derivar sem python-chess) |
| Tronco `detection/*`, `board_detection.py`, `preprocess.py` | detector híbrido e constantes medidas (recall 0,9913) | médio |
| Tronco `pdf_substituicao.py` | geometria `/Rotate`/CropBox, URL do lichess | médio |
| Tronco `piece_classifier.pt` / `char_classifier.pt` | classificadores (13 e 314 classes) | médio (D2) |
| Caïssa `cipher.py`, `legality_repair.py` | cifra por livro; reparo por legalidade | médio/difícil |
| Caïssa IR + `fidelity.py` | forma do `ChessDoc`; teste por releitura | médio |
| Skill `visualizador-pdf-diagramas` | regras de overlay (menor caixa vence, folga de 4 px, recusar caixa de outra página, ordem única) | fácil |

**Não se reaproveita:**
- Tesseract fine-tunado, Paddle e Surya.
- A GUI Qt.
- `net_correction.py`: rede.
- Banco de partidas comercial.
- PNGs de peças sem origem.
- O segmentador de caracteres do PyBoxEditor: é um terceiro repo, então reimplementar.

---

## 1. Decisões do usuário (tomadas em 2026-10-15)

**D1 — Relicenciar o código próprio como `MIT OR Apache-2.0`.** ✅
- O termo e o inventário por arquivo ficam em `plan/chess/provenance.md`: quem escreveu, cabeçalhos "Origem:" e imports de python-chess.
- O relicenciamento cobre **só** o que o usuário escreveu.
- Trechos de terceiros continuam excluídos e são re-derivados da especificação.
- Lógica que hoje se apoia em python-chess é reescrita sobre `cozy-chess`, sem copiar a estrutura.

**D2 — Não distribuir pesos, só o reconhecimento.** ✅
- O repositório traz o **código** de detecção e leitura.
- Os pesos (`chess-pieces.rten`, `chess-glyphs.rten`) ficam na máquina do usuário:
  - desktop: `$PDFCRAFT_MODELS`;
  - web: escolha de arquivo.
- Nenhum `[[fetched]]` de modelo de xadrez. `cargo xtask models` não os baixa.
- Sem modelo: modo "só detectar".
- O antigo D2b (pesos sintéticos distribuíveis) sai do escopo.

**D3 — Repositório independente.** ✅ Criado: `DarcioAlberico/pdfcraft-xadrez` (privado).

*Fluxo de branches:*
- `main` é a branch de integração **nossa**.
- Cada passo vai num branch `feat/chess-<id>` e entra no `main` por PR no nosso repositório.
- Sincronizar com o upstream por **merge**, não por rebase, porque o `main` já é publicado: `git fetch upstream && git merge upstream/main`, semanal ou quando precisar de algo novo, lendo cada conflito.
- Os ganchos nos arquivos disputados (`ui-egui`, i18n, `engine/src/lib.rs`, `automation/src/tools.rs`) continuam **mínimos** para os merges seguirem baratos.

*PRs ⬆ ao upstream:*
- Continuam possíveis, mas exigem um fork do GitHub separado do `storytold/pdfcraft`, com um branch limpo a partir de `upstream/main`.
- Não são pré-requisito de nada aqui.

*Regras do repositório:*
- O repositório é nosso: as entradas de xadrez vão direto no `parity/acrobat-features.toml`.
- O `AGENTS.md`/`CLAUDE.md` herdados continuam valendo como regra da casa (sem pânico, camadas, wasm, ferramentas headless).

*CI:*
- Actions desligado.
- Os portões da §4 rodam **localmente** antes de cada merge.
- Se quiser CI, religar só o `ci.yml` em Linux. Os outros workflows (FreeBSD, ARM64, release) gastam minutos sem necessidade.

*Marca ArtCraft* (`docs/brand/LICENSE-brand.txt`):
- Uma cópia privada pode mantê-la.
- **Antes de publicar ou distribuir** qualquer coisa, o passo **S0b** é obrigatório.

**D4 — Peças e fontes: usar as atuais (Merida, cburnett) por enquanto.** ✅

> **Revista em 2026-10-25 por D13** (`docs/chess/plano-pecas-tabuleiros-lances.md` §4): conjuntos de licença aberta permitida por `AGENTS.md` §1.2 (chessnut, rhosgfx, fantasy, spatial, celtic, kiwen-suwi, firi, totoy, papercut, shapes) podem vir junto do programa, com `ATTRIBUTION.toml`. Merida e cburnett continuam só da máquina do usuário.

*Como entram sem contaminar a licença:*
- Merida é proprietária e cburnett é GPL, portanto **nenhuma das duas é commitada ou embutida no binário**.
- O app as carrega **em tempo de execução, da máquina do usuário**, como o tronco já faz com `CVOFF_MERIDA_FONT` e o pdfcraft com `system_fonts.rs`.

*`PieceSource`, resolvido nesta ordem:*
1. `$PDFCRAFT_CHESS_PIECES`, apontando para um TTF de diagrama (layout do `chess::catalog`) ou para uma pasta com 12 SVG/PNG;
2. a Chess Merida instalada nas pastas de fontes do sistema;
3. a pasta `packaging\assets\piece_images` da Caïssa, se configurada.

*Sem nenhuma delas:*
- detecção, leitura, links e exportação de FEN/PGN funcionam;
- só "substituir diagrama" e as imagens de diagrama na exportação pedem para configurar as peças.

*Trocar pela fonte própria* (quando o usuário decidir) é plugar outro `PieceSource`, sem mudar o resto.

*Fonte de figurinas para HTML/EPUB/camada de texto:* mesmo esquema. Usa um arquivo local configurado (`$PDFCRAFT_CHESS_TEXT_FONT`, p. ex. DejaVu Sans ou a do sistema), sem depender do craft-fonts.

*Aviso registrado:* diagramas e figurinas gerados com Merida/cburnett ficam **dentro dos PDFs/EPUBs exportados**. Para uso pessoal não há problema. Para **publicar** livros, trocar antes pelo conjunto próprio.

*Testes:* sempre com o conjunto geométrico gerado em código, que não depende de nenhum arquivo externo.

### S0b — Remover a marca ArtCraft (obrigatório antes de publicar ou distribuir)
- **Por que:** `docs/brand/LICENSE-brand.txt` exige que versões modificadas **publicadas ou distribuídas** removam os logos. Enquanto o repositório for privado e de uso pessoal, pode esperar.
- **Tarefas:**
  - apagar `docs/brand/*.svg|png` e as 8 entradas do `ATTRIBUTION.toml`;
  - trocar o wordmark e o mark em `crates/ui-egui/src/widgets.rs:502-516` por texto;
  - limpar `README.md` (linhas 4-5, 75, 579-587) e `NOTICE`;
  - refazer as screenshots que mostram o mark (`ATTRIBUTION.toml:3107`, `xtask/src/screenshots.rs:113`);
  - manter o teste de política em `xtask/src/assets.rs:556`;
  - escrever no README "baseado no PdfCraft da equipe ArtCraft", em texto simples.
- **Verificação:** `cargo xtask assets` e um screenshot do About.
- **Reversão:** reverter o commit.

---

## 2. Arquitetura-alvo

```
L0  chess        (novo, standalone; dep. externa só cozy-chess)
      placement (tipo próprio, aceita posições ilegais) · fen · rules::check_position
      san (18 idiomas, figurinas) · nag · lichess::analysis_url
      recognize::{decode, orient, side_to_move}   ← recebem matriz 64×13
      catalog::{diagram_fonts, figurine_fonts}    ← mapas de CÓDIGO de fonte
      draw::board(placement, &PieceSet, style) -> Vec<Shape>  ← fonte ÚNICA do desenho; svg::to_svg
        PieceSet = 12 contornos já em Shape; chess NÃO lê arquivos (wasm, L0)
L6  engine::chess::pieces::PieceSource (D4): TTF de diagrama (contornos via crate de fontes MIT/Apache)
      | pasta de 12 SVG/PNG | Merida instalada → PieceSet. Nada disso é commitado.
L3  render       TextGlyph ganha font + code (⬆)
L4  edit         paths::page_paths(doc, page) -> Vec<PathItem> (⬆, código novo)
L4  chessvision  (novo; deps: chess, render, rten, rten-imageproc, image — NADA do L4)
      detect::{embedded(&[ImageBox]), contour(rgb), font_diagram(&PageText), vector(&[PathItem])}
      merge · order::reading_order (lugar ÚNICO) · board::read · glyphs · chess_text · doc (ChessDoc)
      models::{ChessModels::find, from_bytes}
L4  export       Page.diagrams: Vec<Diagram> (novo campo) · epub() · markdown() · pgn()
L6  engine       chess.rs: adapta PageImage/PathItem → chessvision, cache, jobs,
      Edit::AddContent{page, ops} (⬆ genérico), ReplaceDiagram / AddDiagramLinks → Batch
      OfficeFormat::{Epub, Markdown, Pgn}
L7  automation   chess_* · ui-egui chess_ui.rs
```

**Coordenadas.**
- `DiagramCandidate.bbox_user` fica em **espaço do usuário do PDF** (o de `PageImage.rect` e `AddLink`).
- Uma função única `chessvision::coords` converte de e para o espaço de visão (`TextGlyph.rect`) e o de exibição (`AddText`).
- Testada com `/Rotate` 0/90/180/270 e CropBox deslocada.
- O DPI de rasterização vai **guardado** em cada leitura.

**Regras que atravessam tudo.**
- O reconhecedor devolve a lista **na ordem do detector**.
- Confiança mínima, não média: `UNCERTAIN_SQUARE_THRESHOLD = 0.90`, gate `ACCEPT_MIN_CONFIDENCE = 0.80`.
- **Leitura reparada pelo decodificador tem confiança ≤ 0,5** (`decode.py:318-332`), portanto nunca passa no gate e vai para revisão (S17). Isso é comportamento esperado, não bug.
- Links só para posição `Legal`.
- Sem TTA e sem temperatura aplicada.

---

## 3. Grafo de dependências e ondas

```
S0 ─► S1 ─┬─► S2 ─┬─► S3 ───────────────┐
          │       ├─► S4 (draw) ────────┼──────────────► S10 ─┐
          │       ├─► S12 ◄── S6a       │                      ├─► S17
          │       └─► S11 ◄── S9        │                      │
          ├─► S5 (modelos) ─────────────┼─► S8 ─► S9 ─────────► S16 ─┘
          ├─► S6a (font/code ⬆) ─► S7c ─┤          ▲
          ├─► S6b (paths ⬆) ────► S7c   │          │
          ├─► S7a (embutidas) ──────────┤          │
          └─► S7b (contorno) ───────────┘          │
S5 + S12 ─► S13a ─► S13b ─► S13c        S13d (⬆) ──┤ (S16 "gravar camada")
S3 + S8 + S12 + S13c ─► S14 ─┬─► S15a ─┬─► S15b, S15c, S15d   (S15a usa S4)
                             └─► S16
tudo ─► S18
```

Arestas explícitas:
- S7c ← S2, S6a, S6b
- S8 ← S3, S5, S7a, S7b, S7c
- S10 ← S4, S9
- S11 ← S2, S9
- S12 ← S2, S6a
- S13a ← S5, S12
- S14 ← S8, S12, S13c
- S15a ← S4, S14
- S16 ← S9, S14, S13d
- S17 ← S10, S11, S16

| Onda | Passos (worktree e `CARGO_TARGET_DIR` próprios por agente) |
|---|---|
| 1 | S1 |
| 2 | S2 ∥ S5 ∥ S6a ∥ S6b ∥ S7a ∥ S7b |
| 3 | S3 ∥ S4 ∥ S7c ∥ S12 ∥ S13d |
| 4 | S8 ∥ S13a |
| 5 | S9 ∥ S13b |
| 6 | S10 → S11 (em série: mesmos arquivos `engine/src/chess.rs` e `tools.rs`) ∥ S13c |
| 7 | S14 |
| 8 | S15a ∥ S16 |
| 9 | S15b ∥ S15c ∥ S15d ∥ S17 |
| 10 | S18 |

**Tier de modelo:**
- **mais forte:** S2, S7b, S7c, S9, S10, S13a, S14, S16;
- **padrão:** demais.

---

## 4. Invariantes após **todo** passo

Só nos crates tocados, sob `timeout`, com `-j 2`:

```sh
cargo fmt --check
cargo clippy -p <crate> -j 2 --all-targets -- -D warnings
cargo test   -p <crate> -j 2
cargo check  -p <crate> --target wasm32-unknown-unknown
cargo xtask layers && cargo xtask assets && cargo xtask parity
cargo xtask deny            # sempre que mudar dependência
```

- Crate novo começa com o `#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, …)]` do AGENTS.md §4.
- Fixtures **sintéticas** geradas em teste. Nenhum PDF de livro, modelo ou fonte commitado.
- Todo teste novo visto **falhando** com a correção revertida.
- Toda ferramenta nova tem teste em `automation/tests/automation.rs`, com `--root` respeitado.
- Toda string nova de UI vai em `en.tsv`, `pt-br.tsv` **e** `ja.tsv`.
- Métricas (recall, CER, tempo) só valem com **comando exato + saída + máquina** registrados na §8.
- Sem CI (Actions desligado): estes portões são o **único** gate antes do merge no `main`.
- Branch de passo rebaseado sobre o **nosso** `main` antes do PR. O `upstream` entra por merge (D3), lendo cada conflito.

---

## 5. Passos

> Formato: **Contexto** · **Tarefas** · **Verificação** · **Saída** · **Reversão**.
> Branch `feat/chess-<id>` a partir de `main` (do `DarcioAlberico/pdfcraft-xadrez`). PR para o nosso `main`. Commit `CH<id>: …`. ⬆ = poderia virar PR ao upstream (opcional, D3).
> Para PR de até ~800 linhas: se passar disso, dividir (§7).

### S1 — Esqueleto dos crates
**Contexto.**
- Evita que passos paralelos briguem por `Cargo.toml` e `TABLE`.
- `chess` = `Class::Standalone(0)`; `chessvision` = L4, sem dependência de outro L4.

**Tarefas.**
- `crates/chess` e `crates/chessvision` com README, lints `deny` e módulos vazios (`placement`, `fen`, `rules`, `san`, `nag`, `lichess`, `recognize`, `catalog`, `draw`; `detect`, `merge`, `order`, `coords`, `board`, `glyphs`, `chess_text`, `doc`, `models`).
- Entradas no `TABLE`.
- `cozy-chess` (sem a feature `pext`), `rten`, `rten-imageproc`, `image` (`default-features = false`).
- Copiar o plano e o `provenance.md` para `docs/chess/`.

**Verificação.** Invariantes da §4.
**Saída.** Workspace verde com os dois crates.
**Reversão.** Apagar as pastas e as linhas do `TABLE`.

### S2 — `chess`: posição, regras, notação, lichess, catálogos
**Contexto.**
- O `Board` do cozy-chess rejeita posição ilegal, mas um diagrama lido pode ser ilegal. Por isso há um `Placement` próprio ([[Option<Piece>; 8]; 8]) e conversão para cozy só quando for legal.
- `check_position` sem lado a jogar: tenta os dois lados → `Legal`, `IllegalByTurn{side}` ou `FatallyIllegal(reason)` (dois reis, peão na 1ª/8ª, >8 peões, excesso sem promoções possíveis).

**Tarefas.**
1. `Placement`, `Fen::{parse, format}` tolerantes.
2. `rules::check_position`.
3. `san::{parse, format}(lang)` em 18 idiomas e figurinas, sobre a geração de lances do cozy.
4. `nag` com aliases de editora antes do NFKC.
5. `lichess::analysis_url(placement, side, orientation)`, com percent-encoding próprio.
6. `catalog` com os mapas de **código** das fontes de diagrama e de figurina (dados da Caïssa `font_catalog.py`, reescritos).

**Verificação.**
- Propriedade: FEN ida e volta; SAN ida e volta para todos os lances legais de 1.000 posições aleatórias × 18 idiomas.
- Casos de `check_position` à mão.

**Saída.** `pdfcraft-chess` sem dependência de PDF.
**Reversão.** Reverter o commit; o crate continua vazio.

### S3 — `chess::recognize`
**Contexto.**
- Matriz `(64, 13)` em ordem a8..h1, classes `empty P N B R Q K p n b r q k`.
- Algoritmos re-derivados do tronco: busca de custo uniforme sobre trocas, `MAX_DECODE_CHANGES = 6`; cascata de orientação; lado a jogar texto → legalidade → padrão, com a fonte registrada.

**Tarefas.**
1. `Probabilities::new` validada (forma, finitude, normalização) → `Err` se inválida.
2. `decode` com orçamento por **contagem de nós**: determinístico e compatível com o wasm.
3. `orient`, uma função por regra.
4. `side_to_move(placement, caption, lang)`.
5. `BoardReading{fen, min_confidence, uncertain, orientation, side_source, repaired}`.

**Verificação.**
- Matrizes sintéticas geradas no teste: K/Q trocados (figurina alemã), promoção dupla, 180°.
- Teste explícito: leitura reparada → `min_confidence ≤ 0.5` → reprovada no gate.

**Saída.** FEN legal e explicável a partir de qualquer classificador.
**Reversão.** Reverter o commit.

### S4 — `chess::draw`: desenho do tabuleiro (fonte única)
**Contexto.**
- O mesmo desenho alimenta três destinos: conteúdo PDF (S10), SVG para HTML/EPUB (S15) e PNG para DOCX/RTF/Markdown (S15, via PDF de uma página temporário + `PageRenderer`).
- As peças vêm do D4. Os testes usam o conjunto geométrico gerado em código.

**Tarefas.**
1. `draw::board(placement, &PieceSet, BoardStyle{size, colors, border, coords, flip}) -> Vec<Shape>` (retângulos e paths cúbicos em unidades abstratas) e `svg::to_svg`.
2. `PieceSet::geometric()` gerado em código, para testes e como último recurso.
3. **S4b (engine)** `PieceSource` (D4) resolvido nesta ordem:
   - `$PDFCRAFT_CHESS_PIECES` (TTF de diagrama + layout do `chess::catalog`, ou pasta de 12 SVG/PNG);
   - Chess Merida nas pastas de fontes do sistema;
   - pasta configurada da Caïssa.

   Contornos do TTF via crate de fontes já no workspace (conferir com `cargo xtask deny`). SVG via `usvg` (conferir licença). PNG vira imagem, sem vetor. Sem fonte → `PiecesMissing`, com mensagem dizendo como configurar. **Nenhum arquivo de peça entra no repositório.**

**Verificação.** Snapshot do SVG; contagem de shapes por FEN; propriedade: a peça cai dentro da sua casa.
**Saída.** Desenho determinístico, sem PDF.
**Reversão.** Reverter o commit.

### S5 — Modelos `.rten`
**Contexto.**
- O tronco tem `onnx_export.py` (opset 17) só para o `piece_classifier`. O `char_classifier` (314 classes, 32×32) precisa de exportação **fora do repo**.
- No wasm não há sistema de arquivos.

**Tarefas.**
1. *(No tronco, fora deste repo)* Exportar os dois modelos para ONNX, converter com o `rten-convert` casado com o rten 0.26 e gravar JSON de metadados (classes, entrada, normalização, `arch_version`).
2. `ChessModels::find()` no desktop, igual a `Models::find`, e `ChessModels::from_bytes(model, meta)` no web. Valida a forma ao carregar.
3. `ModelState::Missing` → modo "só detectar", nunca erro fatal.

**Verificação.** Modelo minúsculo gerado no teste (não commitado), paridade de logits ≤ 1e-4 contra valores gravados no teste; ausência → modo detectar.
**Saída.** "Compila no wasm; carrega por bytes."
**Reversão.** Reverter; o app segue sem modelos.

### S6a — ⬆ `TextGlyph` com fonte e código bruto
**Contexto.**
- Figurinas em camada de texto saem como `2.l0xd4`.
- Os catálogos mapeiam o **código** do caractere, mas o `text` atual já passou pelo ToUnicode do hayro.
- Há 15 literais `TextGlyph {}` no workspace, inclusive em `ui-egui` e `automation`.

**Tarefas.**
1. **Spike:** confirmar que o `Glyph` do hayro expõe o nome da fonte e o código. Se não expõe, registrar na §8 e projetar a alternativa (ler da `/Font` via `edit`/`fonts` no engine).
2. Campos `font: Option<Arc<str>>` (prefixo de subset removido) e `code: Option<u32>`, mais um construtor `TextGlyph::new`. Migrar os 15 literais.

**Verificação.** PDF sintético com fonte nomeada: o glifo traz nome e código.
**Reversão.** Reverter o commit (campos opcionais).

### S6b — ⬆ Inventário de paths vetoriais em `edit`
**Contexto.**
- Não existe um walker público.
- O de `redact` não serve: é um reescritor e depende de `fonts::pdf::Metrics`, e uma aresta `content → fonts` (L2 → L2) não é permitida.
- É **código novo**, ao lado de `editable_objects` (`edit/src/objects.rs:54`).

**Ponto de partida (achado no S1):** `crates/measure` já interpreta paths pintados e Form XObjects aninhados, com transformações do estado gráfico e limites (20.000 segmentos, 32 Forms aninhados, 256 `q`; `crates/measure/README.md`). Avaliar se essa extração pode ser **movida para `edit`** e compartilhada (`measure` passa a consumi-la) antes de escrever uma nova. `measure` → `edit` é L4 → L4; mover o código para baixo (para `edit`) evita a aresta lateral.

**Tarefas.** `edit::paths::page_paths(doc, page) -> Vec<PathItem{bbox_user, fill, stroke, subpaths}>`:
- percorre `q/Q`, `cm`, Form XObjects (profundidade limitada e conjunto de visitados);
- teto de itens por página.

**Verificação.** Fixtures com grade 8×8 em paths, Form aninhado e ciclo de Form.
**Reversão.** Reverter (API nova, sem consumidores).

### S7a — Detecção: imagens embutidas
**Contexto.**
- `chessvision` recebe `&[ImageBox{bbox_user, pixels?}]`; o engine adapta a partir de `PageImage`.
- Constantes do tronco: `MAX_PAGE_COVERAGE = 0.70`, `TILE_GAP_PT = 2.0`, lado ≥ 72 pt, aspecto ±0,20, `trim_to_grid` (periodicidade 8×8 ≥ 0,70).

**Tarefas.**
- `detect::embedded`, com junção de ladrilhos e corte à grade.
- `coords` e testes de `/Rotate`/CropBox.

**Verificação.** Imagens sintéticas (tabuleiro, scan de página inteira, ladrilhos).
**Reversão.** Reverter.

### S7b — Detecção: contorno na página rasterizada
**Contexto.**
- Sem OpenCV: `rten-imageproc` (`find_contours`, `simplify_polygon`, `min_area_rect`) + limiar adaptativo próprio (41/8, três passadas).
- Score = geometria × (0,55 + 0,45 × textura). `MIN_AREA_FRACTION = 0.004`, `ASPECT_MAX = 1.62`, `DEDUPE_IOU = 0.9`.
- Pré-processamento do tabuleiro inteiro: deskew ≤3°, flat-field, supressão de trama, CLAHE.

**Tarefas.** `detect::contour(rgb, dpi)`, `board::preprocess`, `board::warp` (800 px), `fit_inner_board`. Constantes num `consts.rs` citando a medição de origem.
**Verificação.**
- Páginas sintéticas desenhadas com `chess::draw` (S4 ainda não existe: usar uma grade 8×8 gerada no teste).
- **Benchmark local** contra os livros do tronco, com o comando e a saída registrados: recall ≥ 0,98 antes de seguir.

**Reversão.** Reverter.

### S7c — Detecção: fonte de diagrama, vetores, fusão e ordem
**Tarefas.**
1. `detect::font_diagram(&PageText)`: glifos com `font` ∈ catálogo → 8×8 → `Placement` **sem modelo**.
2. `detect::vector(&[PathItem])`: grade de 64 retângulos alternados.
3. `merge`: IoU 0,5, prior de tamanho ±30%, descarte de faixas.
4. `order::reading_order` (lugar único).
5. `DiagramCandidate{bbox_user, source, page, dpi, score, placement?}`.

**Verificação.** Fixtures com fonte de teste gerada (nunca Merida real), grade vetorial e as três fontes sobrepostas.
**Reversão.** Reverter.

### S8 — Leitura de tabuleiro → FEN
**Contexto.**
- Recorte → 64 casas → 64×64 cinza → **uma** inferência com 128 casas (0° e 180°) → S3.
- Candidatos de fonte ou vetor que já trazem `placement` pulam o modelo.

**Tarefas.** `board::read(page_rgb, candidates, &PieceModel) -> Vec<BoardReading>` em lote, **na ordem do detector**.
**Verificação.**
- Tabuleiros sintéticos com o conjunto geométrico, mais uma rede minúscula treinada no teste (a ideia é testar o encanamento, não a acurácia).
- Benchmark local com o modelo do usuário (exato ≥ 0,99 no split do tronco), registrado.

**Reversão.** Reverter.

### S9 — Engine + automação: varredura de diagramas
**Contexto.** Padrão de `engine/src/ocr.rs`: thread no desktop, inline no wasm. O cache por `(doc, página, ScanParams)` é descartado quando um parâmetro diverge.
**Tarefas.**
- `engine/src/chess.rs`:
  - `ChessScanSettings`;
  - `Session::chess_scan_job` / `ChessScanJob::run`;
  - `Session::chess_page`;
  - adaptadores `PageImage → ImageBox` e `page_paths → PathItem`.
- Ferramentas `chess_detect`, `chess_read`, `chess_status`.
- CLI `pdfcraft-cli chess scan`.
- Entrada no `parity/acrobat-features.toml` (D3).

**Verificação.** Teste ponta a ponta com PDF sintético e `--root`; `pdfcraft-cli run --script` + `page_render`.
**Reversão.** Reverter (sem efeito em outras ferramentas).

### S10 — ⬆ `Edit::AddContent` + substituir diagrama
**Contexto.**
- `ImageEdit::Replace` exige bytes de arquivo de imagem, ou seja, raster.
- Para um tabuleiro **vetorial**, criar o `Edit::AddContent{page, ops}` genérico sobre `edit::add_content` (útil ao upstream).
- Diagrama-imagem: `ImageEdit::Delete` + `AddContent`. Contorno, fonte ou vetor: retângulo de cobertura + `AddContent`. Tudo num `Batch`.

**Tarefas.**
1. `Edit::AddContent` com desfazer.
2. Engine: `Shape` → operadores PDF.
3. `Edit::ReplaceDiagram{page, index, placement, style}` → `Batch`.
4. Ferramenta `chess_replace_diagram` (`destructive`, `dry_run`).

**Verificação.** Substituir → salvar incremental → reabrir → `chess_read` dá a mesma FEN; desfazer → salvar de novo → **grafo de objetos igual** ao original.
**Reversão.** Reverter; `AddContent` fica se já tiver ido para o upstream.

### S11 — Links do lichess
**Contexto.**
- `AddText` é Helvetica/WinAnsi: o rótulo fica em ASCII ("Analisar no lichess").
- O link vai no espaço do usuário. Só para posições `Legal`.

**Tarefas.**
- Posição do link: abaixo do bbox; se não couber, ao lado (regra testada).
- `Edit::AddDiagramLinks{items, style}` → `Batch` (texto + `AddLinks`).
- Lote "linkar todos os legais".
- Ferramenta `chess_add_lichess_links`.

**Verificação.** `links.list` acha as URIs; posições ilegais são puladas e informadas.
**Reversão.** Reverter.

### S12 — Texto de xadrez a partir da camada de texto
**Contexto.**
- Com `font` e `code` (S6a), mapeia-se o código pelas fontes de figurina do catálogo.
- Avaliar a camada: proporção de mapas mortos e Type0 sem ToUnicode. Camada ruim → página marcada "precisa de OCR".

**Tarefas.**
- `chess_text::from_page_text(&PageText) -> Vec<ChessRun{Text | Move{san, figurine} | Nag | MoveNumber}>`, com bbox.
- Detecção do idioma das letras de peça.

**Verificação.** PDFs sintéticos com fonte de teste que imita um layout de figurina.
**Reversão.** Reverter.

### S13a — Spike de OCR de xadrez (medição antes do desenho)
**Contexto.**
- `ocrs` lê só ASCII + €, e `Word` não tem confiança por caractere.
- O modelo de 314 classes foi treinado com recortes do segmentador do tronco, e o split "NÃO por livro" (`char_meta.json`) deixa o benchmark otimista.

**Tarefas.** Em código descartável, medir a CER nas 744 linhas da Caïssa com três opções:
- (i) `ocrs` puro;
- (ii) palavras do `ocrs` + segmentação por projeção + classificador de glifos;
- (iii) segmentação própria de linha/caractere + classificador.

Registrar comando e números na §8 e escolher o desenho.
**Saída.** Decisão documentada.
**Reversão.** Nada vai para o `main`.

### S13b — OCR de xadrez: implementação escolhida
**Tarefas.** `chessvision::glyphs` + orquestração no engine (`recognize_chess`), com alternativas por caractere quando o modelo as dá. Meta: CER ≤ 2,12% (Caïssa) e token de lance ≥ 0,95, medidos e registrados.
**Reversão.** Reverter; o OCR Latin continua.

### S13c — Cifra por livro e reparo por legalidade
**Tarefas.**
- `BookCipher` aprendida por documento, provada por replay e guardada nas preferências do documento.
- Reparo por legalidade só com contexto de partida (FEN do diagrama anterior).

**Verificação.** Texto sintético com ♕ trocado por `W` de forma sistemática.
**Reversão.** Reverter.

### S13d — ⬆ Camada de texto OCR com Unicode
**Contexto.**
- `text_layer` é Helvetica/WinAnsi, então "♘" vira "N".
- Fonte Type0 invisível + ToUnicode, com a fonte vinda do **craft-fonts**. Sem ela, cair para letras SAN e avisar.
- Fecha parte do gap §7 ("editable-text OCR output").

**Verificação.** Buscar "♘" acha a palavra com craft-fonts; sem craft-fonts, o teste pula e o fallback é testado.
**Reversão.** Reverter.

### S14 — `ChessDoc` + partidas/PGN
**Tarefas.**
- `doc::{ChessDoc, ChessPage, Block::{Heading, Paragraph, Diagram}, Inline::{Text, Move, Nag, Link}}`, cada item com `source_rect` (espaço do usuário).
- Construção a partir de `reading_blocks` + S12/S13 + S8, na ordem única.
- `games`: árvore de variantes validada com cozy-chess a partir da FEN anterior.
- PGN.
- serde JSON.
- Ferramenta `chess_document`.

**Verificação.** Página sintética → `ChessDoc` esperado; PGN reparseado igual.
**Reversão.** Reverter.

### S15a — Exportação: modelo + HTML
**Contexto.** `Block` é struct: acrescentar `Page.diagrams: Vec<Diagram{fen, rect, svg, png, link}>` (`Page` deriva `Default`) e runs inline num campo novo com `Default`, sem quebrar os literais existentes.
**Tarefas.**
- Ponte `ChessDoc → export::Page`.
- HTML: SVG inline, `data-fen`, `alt` = FEN, link abaixo, figurinas Unicode, **sem JS**.
- `OfficeFormat` ganha as variantes novas.
- Parâmetros `include_links`, `diagram_mode = svg|png|fen`, `figurines = unicode|letters(lang)`.

**Verificação.** Releitura (ideia de `fidelity.py`): nº de diagramas, FENs e figurinas batem.
**Reversão.** Reverter.

### S15b — ⬆ EPUB 3
**Tarefas.** `export::epub` = `Zip` + XHTML do S15a + `nav.xhtml` + OPF; fonte embutida só com craft-fonts. epubcheck como oráculo externo, se instalado.
**Reversão.** Reverter.

### S15c — DOCX + RTF
**Tarefas.**
- DOCX: figurinas Unicode, diagrama PNG via S4 → PDF temporário → `PageRenderer`, hiperlink.
- RTF: `\uN?` e `\pict\pngblip`.

**Reversão.** Reverter.

### S15d — ⬆ Markdown + PGN
**Tarefas.** Markdown com `![FEN](diagram-N.png)` + link. PGN com partidas + `[FEN]` por diagrama. Entradas novas no `parity/` (não reutilizar `create.from-markdown`, que é importação).
**Reversão.** Reverter.

### S16 — Vista "PDF ⎮ Texto reconhecido" (lado a lado ou abas)
**Contexto.**
- Precedente: `compare_ui::side_by_side` :290. Regras de overlay da skill `visualizador-pdf-diagramas`.
- Tudo em `ui-egui/src/chess_ui.rs`, com ganchos mínimos nos arquivos disputados.

**Tarefas.**
1. Modo `ChessView{layout: SideBySide | Tabs, sync}`.
   - PDF com caixas coloridas por estado (localizado / lido / substituído) e borda externa no selecionado.
   - Texto com figurinas, lances, NAGs e miniaturas.
2. Sincronia nos dois sentidos (bloco ↔ `source_rect`; página ↔ página).
3. Edição do texto reconhecido → `ChessDoc` + `BookCipher`. "Gravar camada de texto" (S13d) é uma ação explícita, com desfazer.
4. Comandos `chess.view.split`, `chess.view.tabs`, `chess.scan`, `chess.link_all`, `chess.replace_all` e grupo "Xadrez" no catálogo.
5. Strings em `en`, `pt-br` e `ja`.

**Verificação.** kittest em `ui-egui/tests/control.rs`; screenshot via `pdfcraft --control` + `pdfcraft-cli ui screenshot`.
**Reversão.** Reverter; os ganchos são poucas linhas.

### S17 — Editor de diagrama e fila de revisão
**Tarefas.**
- Tabuleiro editável (peças, lado a jogar, roques, virar) e casas incertas com a segunda leitura.
- **Fila de revisão** das leituras reprovadas no gate, inclusive as reparadas.
- "Substituir", "Inserir link", "Copiar FEN", "Abrir no lichess" (`open_url`).
- Anterior/próximo na ordem única.

**Verificação.** kittest: editar, substituir, desfazer.
**Reversão.** Reverter.

### S18 — Endurecimento e documentação
- `cargo xtask fuzz --time 300` com imagens e fontes malformadas e matriz com NaN ou forma errada. Cada achado vira teste antes da correção.
- Tempo por página medido e registrado (CPU, máquina citada).
- `docs/chess.md`, READMEs, `ROADMAP.md` do fork.
- Uma varredura `--workspace` com a máquina livre.

---

## 6. Riscos

| Risco | Mitigação |
|---|---|
| Upstream em ritmo de ~130 commits/dia | crates novos, ganchos pequenos, merge periódico de `upstream/main` (D3), quando houver algo útil |
| Sem pesos distribuíveis, terceiros não leem FEN | modo detectar + pesos do usuário (D2a); dados sintéticos (D2b) |
| hayro não expõe fonte ou código | spike no S6a, com alternativa via engine |
| OCR híbrido abaixo da Caïssa | spike S13a antes de implementar; plano B: treinar reconhecedor `ocrs` com alfabeto ampliado (gap §7) |
| Contorno sem OpenCV pior | gate de recall ≥ 0,98 no S7b antes do S8 |
| craft-fonts depende de outro dono | tudo funciona sem a fonte; fallback testado |
| Contaminação de licença | D1 + re-derivação da especificação |

## 7. Protocolo de mutação
- **Dividir** passos acima de ~800 linhas (`S15c1/S15c2`) e registrar na §8.
- **Pular ou adiar:** motivo + o que fica sem cobertura.
- **Reordenar:** só respeitando a §3, atualizando o grafo.
- **Abandonar:** registrar o ponto de parada e o que foi mesclado.

## 8. Registro
| Data | Evento |
|---|---|
| 2026-10-15 | v1 escrita a partir do mapeamento dos três projetos |
| 2026-10-15 | v2: revisão adversarial aplicada (§9). D1–D4 pendentes. |
| 2026-10-15 | v3: decisões tomadas (D1 relicenciar; D2 sem pesos; D3 repositório `DarcioAlberico/pdfcraft-xadrez` privado, Actions desligado, push ao upstream bloqueado; D4 Merida/cburnett carregadas em tempo de execução). `provenance.md` criado: o PDFimport (Wagner Escorcio) é terceiro e fica excluído. S0b (remover a marca ArtCraft) antes de publicar. |
| 2026-10-15 | Titular confirmou: PGN_Live_Editor é todo dele e há permissão de Wagner Escorcio para o PDFimport. Os trechos do PDFimport voltam ao escopo. Pendente só guardar a permissão antes de publicar. |
| 2026-10-15 | **S1 concluído** (branch `feat/chess-s1`): crates `chess` (L0, cozy-chess 0.3.4 MIT, sem `pext`) e `chessvision` (L4: chess + rten + rten-imageproc), com módulos documentados e vazios. `fmt`, `clippy -D warnings`, `test` e `check` wasm32 passam nos dois crates; `xtask layers` OK (39 crates) e `xtask assets` OK. **`xtask parity` falha no próprio upstream** (chave `evidence` duplicada em `parity/acrobat-features.toml:2625`, não causada por nós; esperar a correção do upstream). `cargo-deny` não está instalado: dependência nova conferida à mão (MIT). Toolchain: Rust 1.99 via rustup (winget). |
| 2026-10-16 | **S2–S9 e S13b–S13d concluídos** (PRs #2–#19, branches `feat/chess-<id>`): posição, FEN, regras, SAN em 18 idiomas, NAGs e catálogo de fontes (S2); pós-processamento do reconhecimento (S3); desenho do tabuleiro e SVG (S4, S4b: peças da fonte de diagrama do usuário); modelos ONNX do usuário (S5); `TextGlyph` com fonte e código e inventário de paths (S6a/S6b); detecção por imagens embutidas, contorno, fonte de diagrama e vetores, com fusão e ordem de leitura (S7a–S7c); leitura em FEN (S8); varredura no engine, ferramentas e CLI (S9); texto de xadrez da camada de texto (S12); leitor de figurinas, cifra por livro e camada OCR com Unicode (S13b–S13d). O spike S13a ficou registrado no próprio S13b. |
| 2026-10-18 | **S10, S11 e S14–S18 concluídos** (PRs #20–#32): `Edit::AddContent` e substituir diagrama (S10); links do lichess (S11); `ChessDoc`, partidas e PGN (S14); exportação HTML, EPUB 3, DOCX/RTF, Markdown e PGN (S15a–S15d); painel de diagramas, texto reconhecido ao lado da página (S16); editor de diagrama e fila de revisão (S17); fuzzer, tempos e o guia `docs/chess.md` (S18). |
| 2026-10-18 – 2026-10-19 | Melhorias de leitura medidas em livros reais (PRs #33–#51): páginas e scans em duas colunas, notação algébrica longa, leitura e exportação fora da thread da interface, cifra entre leituras, figurinas e fontes de diagrama em subconjunto, partidas que atravessam diagramas e páginas, colchetes com respostas, soluções para cada lado, coleções de partidas, releitura de scans, lances cortados pelo OCR, problemas numerados (Polgar). #39 corrigiu as chaves duplicadas do `parity` (o `xtask parity` volta a passar). |
| 2026-10-19 | Tabuleiro de análise (#42) com variantes, anotações, comentários e dados da partida (#48; a revisão achou e corrigiu espaços comidos ao digitar, promoção além de `MAX_DEPTH` e um teste que não testava o limite); camada OCR invisível de scans não é mais confiável e a origem do livro pode ser escolhida (#46); peças de uma pasta de 12 PNG, `pieces\` ao lado do executável, traçadas em contornos (#52, completa a D4 para PNG); vez de jogar e número do lance pelo primeiro número em negrito sob o diagrama (#53). |
| 2026-10-19 | Em revisão: comentário que abre uma variante vai para o primeiro lance dela (#54, resto da revisão do #48); **S0b** — logos da ArtCraft removidos, README e NOTICE dizem que é uma versão modificada (#55); sincronização semanal com o upstream, 274 commits (#56, D3), que corrige 6 dos 9 testes herdados que falhavam. |
| 2026-10-19 | **Pendências.** (1) Guardar por escrito a permissão do autor do PDFimport em `docs/chess/permissions/` antes de publicar (ver `provenance.md`): só o titular pode fazer. (2) D4: pasta de peças em SVG e a fonte de figurinas `$PDFCRAFT_CHESS_TEXT_FONT` para HTML/EPUB ainda não existem. (3) Ambiente Windows desta máquina: o teste de pixels do poppler (`organize`) usa o `pdftoppm` do MiKTeX, que se recusa a rodar, e `ui-egui --test content` às vezes cai com violação de acesso quando roda em paralelo (14/14 em série). |
| 2026-10-20 | **D4 completa:** fonte de figurinas do usuário no HTML e no EPUB (`$PDFCRAFT_CHESS_TEXT_FONT` ou uma pasta `fonts` ao lado do executável; `@font-face` restrito por `unicode-range` aos símbolos de xadrez; embutida como data URL no HTML e como arquivo no EPUB) (#59); peças em SVG na pasta de peças, com os caminhos do próprio arquivo via `usvg` (#60). Proveniência: pasta `docs/chess/permissions/` com o passo a passo e um modelo de termo para o autor do PDFimport (#61). **A pendência (1) continua aberta** até a cópia escrita do autor ser guardada. |
| 2026-10-20 | **Tabuleiro de Estudo** (pedido do usuário, à maneira do tabuleiro do chessvision.ai, só pelo que as páginas públicas mostram): aba ao lado de Diagramas e Texto, com o mesmo tabuleiro e a mesma partida do painel Tabuleiro. Montar posição (paleta, posição inicial ou vazia, vez, roques ajustados às peças); setas e círculos com o botão direito, por posição; análise pelo motor UCI do próprio usuário no desktop (`pdfcraft_chess::uci` em L0 para o protocolo, `engine::chess::analysis` para o processo; `PDFCRAFT_CHESS_ENGINE`, pasta `engine` ao lado do executável ou escolhido na sessão; nenhum é distribuído), com barra de avaliação, 1–3 linhas clicáveis e a seta do melhor lance; material. **Abrir no tabuleiro** passa a levar à aba. Sem ferramenta de automação para o motor, de propósito (seria um MCP rodando programas). Conferido com o Stockfish 18 desta máquina (`PDFCRAFT_CHESS_TEST_ENGINE`). |
| 2026-10-20 | **Linha que para no lance impossível.** Medição só do walker (`DocBuilder::add_read_page` + `examples/walk_bench.rs`, sobre 10 documentos lidos em 19/10): 2.036 válidos, 196 ilegais, 2.778 não alcançados — cada ilegal derrubava ~14 lances. **Tentado e descartado:** inferir um lance parecido no lugar do que falhou quando os 2 seguintes cabem em exatamente um (+41 válidos), mas os 6 lances inferidos estavam todos impressos certos (a linha estava no ramo errado, ou um `hg` não fora lido como lance): adivinhar só escondia a causa — mesma conclusão de 19/10. **Mantido:** lance numerado que é exatamente o próximo da linha principal e é legal nela volta para ela depois de uma digressão em prosa, em vez de virar alternativa dentro da digressão: 2.036 → 2.304 válidos (+268), não alcançados 2.778 → 2.509; 13 lances pioram em 2 partidas cuja primeira cadeia de lances é ela mesma uma digressão. Conferido nas páginas: Fischer p. 46, Flores p. 60, Dvoretsky p. 112. |
| 2026-10-20 | **Linha principal pelo peso da fonte.** Cada lance traz o peso impresso (`bold` no JSON, só quando verdadeiro; OCR e documentos antigos não têm). Numa partida com lances dos dois pesos, a linha em negrito é a principal: lance em negrito depois de análise em tipo normal continua do último lance em negrito quando cabe ali (sem número, só se o último em negrito foi jogado), e lance em negrito jogado na linha em negrito a põe em primeiro; caído dentro da análise, não. Releitura de 10 trechos (release `pdfcraft-cli`, modelos do usuário; Karpov e Capablanca como guardados) medida com `walk_bench` com e sem `--plain`: 2.305 → 2.515 válidos, não alcançados 2.509 → 2.329; 217 melhores, 7 piores (todos em ramo já errado: Fischer pp. 33, 35, 56). Conferido nas páginas: Dvoretsky pp. 51–52, 100–101, 108–110, 113, 115, 119; Fischer pp. 37–38, 43–44, 47, 52–54. Ganhos por acaso: Fischer p. 49 (diagrama centrado lido depois da linha curta ao lado; a partida começa na digressão) e p. 55 (diagrama com o lado errado). |
| 2026-10-20 | **Diagrama centrado antes da linha curta abaixo dele.** No Fischer pt-BR o lance seguinte vem na margem esquerda sob o diagrama centrado (`19 … ♗xd5`, x 73–155; diagrama x 159–453): o diagrama só ia antes de linha sob ele ou numa coluna à direita, então vinha depois desse lance, e a partida fechava e recomeçava da análise. Agora vai também antes de linha inteira abaixo dele do mesmo lado da calha (ou do meio da página, sem calha); diagrama ou linha que cruza o meio está dos dois lados. Releitura dos 10 trechos (CLI release do #66 + a correção, contra `boldrun`): 2.515 → 2.948 válidos, todos no Fischer pp. 30–59 (706 → 1.139 de 1.179; não alcançados 414 → 20; 18 → 9 partidas), nenhum lance piora, os outros 9 trechos idênticos (Karpov e Capablanca comparados com releitura nova do CLI do #66). Sem os pesos do negrito (`--plain`): 2.305 → 2.508. Conferido nas páginas: Fischer pp. 30, 31, 33, 34, 38, 47, 52, 56. |

## 9. Revisão adversarial (v1 → v2)
**Críticos corrigidos:**
1. `chessvision` não depende de nenhum L4: a orquestração foi para o engine (L6).
2. `content` é L2; o walker de paths virou código novo em `edit` (S6b).
3. Grafo e ondas reconciliados: S1 de esqueleto, S7c ← S2, fixtures com conjunto geométrico, arestas que faltavam.
4. `Edit::AddContent` genérico + `chess::draw` como fonte única de PDF, SVG e PNG.

**Maiores corrigidos:**
- espaço de coordenadas único (`bbox_user`);
- spike de fonte/código (S6a) e de OCR (S13a);
- modelos por bytes no wasm;
- leitura reparada sempre vai para revisão;
- `ja.tsv` obrigatório;
- parity só no `acrobat-features.toml`;
- `Page.diagrams` em vez de `Block::Diagram`;
- proveniência por arquivo;
- passos divididos com reversão.

**Menores:**
- wasm checado por crate;
- `cargo xtask deny`;
- `rten-imageproc` no lugar de `imageproc`;
- `Placement` próprio no lugar do `Board` do cozy;
- peças contributor-original;
- Noto Sans Math para ⩲;
- rótulo do link em ASCII;
- links só para posições legais;
- `open_url`;
- métricas com comando registrado;
- desfazer comparado por igualdade do grafo de objetos;
- `reading_order` num lugar só;
- exportação sem JS.
