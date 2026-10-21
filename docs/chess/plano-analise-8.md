# Plano de implantação — Análise PDFCraft 8.0 (testador) · v4 (final)

**Status:** v4 · 2026-10-21 · **final**, após três revisões: interface gráfica, usabilidade, exportações e engenharia (§9).
**Origem:** `Analise_PDFCraft_8.pdf`, 13 sugestões de um testador que usou um executável antigo (0.8.0). Cada sugestão foi conferida contra o `main` de 2026-10-21 (`14d57da`, PRs até #97) por três auditorias de código e por capturas do app real dirigido pelo canal de controle (`--control`; cliente em `D:\Python-Chess2\PdfCraft\ux\ui.py`).
**Repositório:** `DarcioAlberico/pdfcraft-xadrez` (Rust, egui). Crates: `chess` (L0), `chessvision`, `export`, `engine` (L6), `ui-egui`, `automation`.
**Público de cada passo:** uma sessão de agente que começa do zero; cada passo traz contexto, tarefas, verificação, critério de saída e reversão.
**Nomes:** os achados das revisões usam o prefixo **A8-** para não colidir com os passos U1–U7 da onda 10.

---

## 0. Leia antes de qualquer passo

- **Regras da casa:** `AGENTS.md`/`CLAUDE.md` — sem pânico; camadas (`cargo xtask layers`); `chess`/`chessvision`/`engine` compilam para wasm (sem IO de arquivo direto no engine); **toda função nova tem ferramenta headless** e entrada em `parity/acrobat-features.toml`; ferramentas respeitam `--root`. Portões de `docs/chess/blueprint.md` §4.
- **Tela de referência: 1366 × 768** (a janela do usuário abre com 1360 × 705 pontos lógicos). Tudo o que um fluxo exige fica visível sem rolar; 1920 × 1080 é o segundo alvo; claro e escuro.
- **Coordenação com a onda 10:** outra sessão faz a U6 em `chess_ui.rs`, `chess_board_ui.rs`, `chess_window.rs`. Os passos que tocam esses arquivos (P1b, P2a, P4, P6, P3b2) só começam **depois do merge da U6**; P1a, P1c, P3a1, P3b1 e P8a podem começar já. Todo passo parte de um `main` atualizado e se atualiza antes do PR.
- **Textos:** string nova no `tl!()` (inglês é a fonte) e em `pt-br.tsv`, `ja.tsv`, `lt.tsv`, `vi.tsv`; textos pt-BR de §2.2.
- **Ícones:** só Lucide do projeto; ícone novo pelo `ATTRIBUTION.toml` + `cargo xtask assets`. Nunca caracteres soltos (✕ ◀ ▶ ⏮ ● ▲ ○ ⋯) em botões.
- **Fontes do usuário** (SemFig, ISChess, Merida) nunca entram no repositório; testes usam Inter de `assets/fonts` com o papel forçado ou arquivos sintéticos.

## 1. O que a auditoria encontrou

### 1.1 As 13 sugestões

| # | Sugestão | Estado no `main` | Causa, com evidência |
|---|---|---|---|
| 1 | FEN poluindo o painel | **Procede** | FEN em cada linha (`chess_ui.rs:462`), linha de 58 pt; repetido no editor (`chess_editor.rs:278`). |
| 2 | Figurinhas em vez de letras no editor | **Procede no editor** | Paleta K Q R B N P (`chess_editor.rs:140-150, 237-250`); o tabuleiro de estudo já usa imagens (`chess_study_ui.rs:326-339`). |
| 3 / 13 | Salvar o diagrama revisado | **Procede — o mais grave** | Sem "confirmar"; correção só em memória, perdida ao trocar/navegar/fechar (`chess_ui.rs:651-653, 718, 730`); "Substituir na página" altera o PDF e dispara nova varredura que apaga a lista inteira (`ScanStamp`, `engine/src/chess.rs:918-928`); ações fora da tela a 1366 × 768; a exportação nem usa a revisão (`export.rs:144` relê tudo). |
| 4 | PGN com diagrama e página | **Parcial** | `PdfPage` (física)/`PdfDiagram`/`PdfPuzzle` existem (`games.rs:941-945`, `markdown.rs:130-139`); sem rótulo impresso, sem título, sete etiquetas em `?`, jogos do tabuleiro sem etiqueta (`chess_board_ui.rs:273-303`); origem em inglês fixo (`markdown.rs:135`). |
| 5 | Clique no diagrama não o identifica | **Parcial** | A rolagem esconde o editor; física × rótulo; só com Selecionar (`canvas.rs:3571`). |
| 6 | Número da página | **Parcial** | Trilho direito (`chrome.rs:761-795`), sem "rótulo · n de N", sobreposto. |
| 7 | Zoom | **Existe, não se acha** | Pé do trilho, % sem moldura (`chrome.rs:687-743`). |
| 8 | Motor | **Existe (#64), escondido** | `chess_study_ui.rs:577-761`; seção fechada; mensagem técnica. |
| 9 | Voltar a um diagrama | **Parcial** | Histórico só por página; abrir no tabuleiro não registra; fila sem aviso no fim. |
| 10 | Página em cor | **Procede** | `chess_ui.rs:428`. |
| 11 | Verde/laranja sem explicação | **Procede** | `chess_ui.rs:33-36`; cor como única pista. |
| 12 | Editor no topo; abrir em aba | **Procede** | `chess_ui.rs:406-414`. |

### 1.2 Achados das capturas e das revisões

**C** crítico · **I** importante · **P** polimento.

| Id | Sev. | Achado | Evidência |
|---|---|---|---|
| A8-V1 | C | Trilho direito sobreposto a 705 pt (alvos uns sobre os outros). | `chrome.rs:670-680` × `685-796` |
| A8-V2 | C | Aba numerada cobre a casa a8; branco ≈ 2,7:1 sobre laranja. | `chess_ui.rs:258` |
| A8-V3 | C | Cores de estado fixas, sem tema escuro; verde ≈ 3,4:1. | `chess_ui.rs:33-36`; `chess_editor.rs:231,280,282` |
| A8-V4 | C | Figurinhas pretas sumiriam no escuro. | `theme.rs` |
| A8-U1 | C | Correções se perdem em qualquer edição; nada vai a disco. | `engine/src/chess.rs:170-177, 918-928` |
| A8-U2 | C | Revisão sequencial ambígua; fim da fila sem aviso. | `queue.first()` |
| A8-U3 | C | Editor só por mouse; leitor de tela ouve o FEN. | `chess_editor.rs:212` |
| A8-U4 | C | Atalhos em conflito; Alt+→ vira página; `Shortcut` sem `alt`. | `canvas.rs:2325-2329`; `commands.rs:57` |
| A8-E1 | C | Dois escritores de PGN divergentes (ordem de etiquetas, saneamento). | `pgn.rs:67-82` × `game.rs:863-882` |
| A8-E2 | C | Diagramas no HTML/EPUB com nome acessível = FEN; `<html>` sem `lang`. | `svg.rs:17`, `html.rs:222, 229, 300` |
| A8-E3 | C | Word/RTF sem fonte por trecho: `Block` tem uma só fonte; o exportador é compartilhado com PDF→Word. | `export/src/lib.rs:19, 98`; `office.rs:121-189` |
| A8-E4 | C | Trocar o sentido de `PdfPage` quebra o contrato de `chess_export_pgn`. | `automation/src/tools.rs:1310`; `doc/tests/board.rs:46` |
| X1 | I | Glifo ausente (✕ → □); caracteres soltos. | `03-editor.png`; `chess_board_ui.rs:1249, 1262-1272` |
| I1–I8 | I | Linha desalinhada; cabeçalho em 3 linhas; progresso cortado; azul ambíguo; dois idiomas de seleção; tingimento; pílulas sem foco; três estilos de aba. | `chess_ui.rs:251-475`; `widgets.rs:132, 552` |
| A8-U5 | I | Sem estado "Conferido"; ilegal sem motivo; clique só com Selecionar; numerações misturadas; "Redesenhar" sem aviso; exportar sem aviso de pendências. | revisão 2 |
| A8-E5 | I | EPUB sem `dc:creator`/`dc:source`, sem metadados de acessibilidade, sem `page-list`; CSS repetido em cada página; identificador muda a cada salvamento; comentário "No font is embedded" desatualizado. | `epub.rs:7-8, 40, 105, 129`; `html.rs:320` |
| A8-E6 | I | Fonte embutida sem checar `OS/2 fsType`. | `text_font.rs` |
| A8-E7 | I | Word: 160 ppi, sem texto alternativo, legenda "FEN:" de 8 pt, link do lichess como texto, sem `w:lang`/`noProof`; RTF só com Helvetica. | `office.rs:28,32,162,166`; `export/src/lib.rs:579, 858` |
| A8-E8 | I | Markdown: alt = FEN, `---` por página, sem front matter, pasta de imagens com caracteres proibidos no Windows. | `markdown.rs:80, 111`; `export.rs:106` |
| A8-E9 | I | Jogos de posição duplicam diagramas do meio da partida; diagramas não exportados somem calados. | `markdown.rs:162` |
| X3 | I | Tabuleiro do painel mais largo que o painel. | `chess_board_ui.rs:742` |
| X4–X6 | P | Painel de ferramentas rouba 270 pt; trilho do PGN sem rótulo; "Exportar…" ambíguo. | capturas |

## 2. Princípios de desenho

1. Uma ação principal por tela, em destaque e sempre visível.
2. **Nada se perde:** correções em disco, sobrevivem a edição, desfazer, nova detecção, salvar, fechar e reabrir.
3. Cor nunca sozinha; texto colorido só com `*_text` (≥ 4,5:1).
4. Mesma numeração em todo lugar ("Pág. 17 · diagrama 1 de 2"), inclusive nos arquivos exportados.
5. Ícones Lucide; todo botão de ícone com nome acessível e dica com atalho.
6. Caber em 1366 × 768.
7. Um idioma visual por conceito (seleção = `accent`; azul não é estado; um estilo de abas).
8. Teclado completo; atalhos sem colisão por escopo.
9. Avisar antes do que altera o arquivo; depois, oferecer desfazer.
10. **Arquivos exportados acessíveis e portáveis:** texto alternativo, idioma, metadados; origem que sobrevive à importação em qualquer programa (comentário legível, não só etiquetas).
11. **Contratos públicos só por adição** (etiquetas PGN, campos JSON das ferramentas).

### 2.1 Especificação visual

**Tokens em `theme.rs`** (no teste `text_is_readable_on_every_surface`):

| Token | Claro | Escuro | Uso |
|---|---|---|---|
| `success` / `_text` / `_soft` | #2D9D5B / #1E7A43 / #E3F4EA | #3FB871 / #6BD39A / #1F3A2B | lido, conferido, corrigido |
| `warning` / `_text` / `_soft` | #E08A1A / #9A5B00 / #FDF0DC | #F0A040 / #F5B969 / #3D2E17 | a revisar, rascunho |
| `danger` / `_text` / `_soft` | #E03E3E / #C42B2B / #FCE5E5 | #F06464 / #F58C8C / #43232A | erro, ilegal |
| `attention` | #7C3AED | #A78BFA | casas duvidosas |

**Estados** (`DiagramState`, um só lugar): Lido `circle-check`/`success` · A revisar `triangle-alert`/`warning` · Não lido `circle-dashed`/`text_faint` · Com erro `circle-x`/`danger` · Conferido `badge-check`/`success` · Corrigido `badge-check`/`success` + marca "editado" · Rascunho = ponto `warning` sobre o ícone.

**Foco:** anel de 2 pt em `accent` nas pílulas. **Tipografia:** 17 · 13 · 12 · 11,5 · 10,5 (maiúsculas); nada abaixo de 11 pt.

**Painel de diagramas:** cabeçalho de uma linha (30 pt) — `split_pill` primário "Detectar diagramas" ▾ (livro todo / nesta página); `file-output`, `layout-grid`, `ellipsis` à direita; progresso com `ProgressBar` 4 pt, "pág. 75 de 703" de largura reservada e `x` "Parar". Chips de filtro (24 pt, 4 pt entre eles) com ícone + "A revisar 40"; ativo `accent_soft` + traço + `check`. Cabeçalho de página (24 pt) "Pág. 17" em `accent_text`, clicável, "· 25 de 703" quando diferir. Linha de diagrama (32 pt): ícone 16 pt, "Diagrama 1", estado à direita, `ellipsis` em hover/seleção/foco/menu de contexto. Lista virtualizada com **`ScrollArea::show_viewport` e somas prefixadas** (cabeçalhos de 24 pt e linhas de 32 pt; `show_rows` exige altura uniforme). "Acompanhar a página".

**Aba "Diagrama":** cabeçalho 36 pt (link "Pág. 17 · diagrama 1 de 2" = "Mostrar na página"; `cpu` Analisar; `chevron-left` · "12 de 40 a revisar" · `chevron-right`); tabuleiro `min(largura − 24, altura − 204)` entre 200 e 420 pt; paleta 2 × 7 de 32 pt com fundo casa clara e `eraser`; "Quem joga" segmentado com texto; "Lance nº"; "Detalhes" (roque por extenso, FEN editável); casas duvidosas tracejadas em `attention` + `circle-help`; **rodapé fixo 52 pt** com chip "Posição legal / ilegal: &lt;motivo&gt;", `ellipsis` e split primário "Confirmar e seguir" ▾ "Só confirmar" (fora da revisão, o inverso; lembra a escolha).

**Marcas na página:** traço 1,5 pt em repouso; hover 8 % + mão; seleção 2,5 pt + anel `accent`; aba numerada fora da caixa com ícone de estado de 10 pt e texto ≥ 4,5:1.

**Barra superior:** zoom (`zoom-out` | % com moldura | `zoom-in` | `maximize-2` ▾) e página ("Pág. [17] de 703" / "Pág. [xvii] · 25 de 703"); trilho direito só com página e histórico, medido.

**Tabuleiro do painel:** lado descontando a barra de rolagem; coordenadas fora das casas ≥ 10 pt; "Quem joga" segmentado; navegação `chevrons-*`/`chevron-*`; dicas e atalhos no trilho do PGN; sem título repetido.

### 2.2 Textos pt-BR

| Controle | Texto |
|---|---|
| Detecção | **Detectar diagramas** ▾ "No livro todo (703 págs.)" / "Nesta página" · "Detectando… pág. 75 de 703" · "Parar (os já encontrados ficam)" · fim "548 diagramas · 40 a revisar" [Revisar agora] |
| Vazio | "Nenhum diagrama detectado ainda. O PdfCraft acha os diagramas e lê as posições. O livro todo leva alguns minutos." |
| Chips | "Todos 548" · "Lidos 468" · "A revisar 40" · "Conferidos 9" · "Corrigidos 12" · "Não lidos 20" · "Com erro 8" · "Nesta página" |
| Dicas | "Posição lida com confiança." · "Houve dúvida em alguma casa. Confira antes de usar." · "Não deu para ler. Monte a posição no editor." · "Você conferiu e manteve a leitura." · "Você corrigiu esta posição." |
| Editor | "Pág. 17 · diagrama 1 de 2" · "12 de 40 a revisar" · "Posição legal" / "Posição ilegal: falta o rei preto" ("Dois reis brancos", "O lado que não joga está em xeque", "Peão na 1ª ou 8ª fileira") · **Confirmar e seguir** ▾ "Só confirmar" (Ctrl+Enter) · "Alterações não confirmadas" |
| 1ª vez | "Clique numa casa para pôr a peça. **Confirmar** guarda a correção sem mudar o PDF." |
| Modal | "Guardar a correção do diagrama 1 (pág. 17)?" · "Você mudou 2 casas e ainda não confirmou." · [Confirmar correção] [Descartar mudanças] [Continuar editando] |
| Redesenhar | "Redesenhar no PDF desenha a posição corrigida por cima do original. Isso altera o arquivo; dá para desfazer com Ctrl+Z até salvar." [Redesenhar] [Cancelar] ☐ "Não mostrar de novo" |
| Sem motor | "Nenhum motor de análise instalado" · "Para analisar, o PdfCraft usa um programa à parte, como o Stockfish (gratuito). Baixe-o e escolha o arquivo stockfish.exe." · [Escolher motor…] [Abrir stockfishchess.org] [Agora não] · "Ou ponha o programa numa pasta engine ao lado do PdfCraft." |
| Página | "Pág. [17] de 703" · "Pág. [xvii] · 25 de 703" · dica "Número impresso no livro; depois, a posição no PDF. Digite e tecle Enter (Ctrl+Shift+N)." |
| Exportar | "Exportar livro de xadrez" · "PGN — partidas" · "EPUB — livro digital" · "HTML — página da web" · "Word (.docx)" · "RTF" · "Markdown" · "40 diagramas ainda não conferidos" [Revisar primeiro] [Exportar assim mesmo] · [Abrir pasta] · "A fonte SemFig só aparece no RTF em computadores que a tenham instalada." |

**Trocas:** "Procurar nesta página" → "Detectar diagramas" · "Vez:" → "Quem joga:" · "Substituir na página" → "Redesenhar no PDF…" · "Voltar à leitura" → "Restaurar a leitura automática" · "Usar a segunda leitura" → "Trocar por bispo branco" · "Roque: K Q k q" → "Roque — Brancas: curto, longo · Pretas: curto, longo" · "Modelos…" → "Modelos de leitura…" · "Copiar FEN" com dica "Código da posição aceito por outros programas de xadrez."

### 2.3 Teclado e acessibilidade

- **Atalhos por escopo** (teste de tabela): Alt+←/→ = histórico; aba Diagrama: PageUp/PageDown diagrama anterior/próximo, Ctrl+Enter "Confirmar e seguir", Ctrl+Z/Ctrl+Shift+Z desfazer/refazer **locais**, Esc devolve o foco à lista (nunca descarta). `Shortcut.alt`; o canvas exige modificadores exatos.
- **Ordem de foco:** cabeçalho → tabuleiro → paleta → Quem joga → Lance nº → Detalhes → segundas leituras → `ellipsis` → Confirmar; foco no tabuleiro ao abrir e após confirmar.
- **Tabuleiro pelo teclado:** uma parada de Tab; setas movem o cursor; letras da notação do idioma da interface (pt-BR R D T B C P; Shift = preta); Delete/Espaço esvazia; AccessKit por casa ("e4, cavalo branco"); paleta por setas.
- **Anúncios:** troca de diagrama ("Diagrama 4 de 40 a revisar, pág. 18, 2 casas duvidosas"); progresso a cada 10 %.
- Alvos ≥ 24 × 24 pt; movimento reduzido via `motion::animated`; avisos com "Desfazer" ≥ 8 s, sem sumir com o ponteiro.

### 2.4 Especificação dos arquivos exportados

**PGN** (um só caminho para os dois escritores):
- Tipo único `PgnOrigin { book_title, page_index, page_label, diagram, of, puzzle, lang }` no crate `chess` (L0), aplicado por `chessvision::doc::pgn::write_game` e por `chess::game::Game::pgn`; teste de equivalência (mesma origem → mesmas etiquetas e mesma ordem: as sete, `SetUp`/`FEN`, extras).
- Etiquetas **só por adição:** `PdfPage` continua física (contrato de `chess_export_pgn`); novas `PdfPageLabel "xvii"` e `SourceTitle "<título>"`. `Event` recebe o título **só** nos jogos de posição (`Event` = `?` hoje), com `Round "-"`; partidas reais mantêm o torneio. `Annotator` vazio.
- Comentário inicial legível **no idioma do livro** (`ChessDoc.lang`): `{Pág. 17 · diagrama 1 de 2}` — o portador universal, já que o ChessBase descarta etiquetas desconhecidas.
- Jogos de posição: opção `positions = all|unused|none` (padrão `unused`, sem duplicar diagramas do meio de partida); na partida, o diagrama marcado com o NAG `$220`; a ferramenta devolve `positions_skipped` com motivos (ilegal, reparado, não lido).
- Corrigir `--` para SAN vazio (`pgn.rs:151`); tokens longos sem estourar 79 colunas; UTF-8 sem BOM.
- **Testes:** golden com livro sintético (roque KQkq, en passant, 23… de pretas, comentário com `}` e `%`, NAG ≥ 140, etiquetas com `"` e `\`); reimportação por `parse` e `Game::from_pgn` (FEN inicial, linha principal e etiquetas iguais); oráculo externo `python -I -c "import chess.pgn…"` (python-chess é GPL: só como processo externo; o teste pula sem Python).

**HTML / EPUB:**
- SVG com `<title id>` "Diagrama 1, pág. 17: brancas jogam" e `<desc>` com as peças por lado ("Brancas: Rg1, Df3; peões a2, b2…"), `aria-labelledby`, `data-fen` mantido; `figcaption` igual ao comentário do PGN; `<html lang>`.
- EPUB: `dc:creator`/`dc:source`; metadados de acessibilidade (`schema:accessMode`, `accessibilityFeature`, `accessibilityHazard`, `accessibilitySummary` — Lei Europeia de Acessibilidade, desde 28/06/2025); `page-list` no `nav` e `pagebreak` com os **rótulos impressos**; "Page …" e "Diagram … (not read)" localizados; `styles.css` único no manifesto; identificador por `DocKeys.id0` (estável entre salvamentos); `.review` desligado por padrão; comentário de `epub.rs:7-8` corrigido.
- Fontes: recusar embutir quando `OS/2 fsType` for "Restricted", com o motivo no JSON `text_font`.
- Orçamento: HTML de 548 diagramas ≤ 15 MB (medido); se estourar, `<symbol>`/`<use>` por peça.
- **Testes:** XHTML bem-formado (parser XML); golden com relógio fixo; `epubcheck` como teste `#[ignore]` com o comando documentado.

**Word / RTF** — módulo novo `pdfcraft_export::flow` (o caminho PDF→Word fica intocado, com seu golden):
- `Para`, `Run { font, lang, no_proof }`, `Figure { png, svg?, alt, caption }`, `Hyperlink`; escritores DOCX e RTF próprios.
- Figurinhas e sinais em trechos com SemFig/ISChess; `w:lang` no idioma do livro; `w:noProof` nos lances; `\fonttbl` com as fontes usadas; aviso quando a fonte não estiver instalada (RTF).
- Diagramas a 300 ppi (900 px em 216 pt) ou SVG no DOCX (`asvg:svgBlip`) com PNG de reserva; `wp:docPr descr` com o texto alternativo; legenda no estilo `Caption` com `keepNext`; link do lichess como hiperlink.
- **Testes:** DOCX abre sem reparo (validação do pacote e do XML); `descr` e `Caption` presentes; RTF com a tabela de fontes certa.

**Markdown:** alt text descritivo (como o `<desc>`); um separador por capítulo, não por página; front matter `title`/`lang`; nome da pasta de imagens saneado para o Windows (`:` `?` `*` …).

## 3. Passos

Branches `feat/chess-a8-<id>`. Tamanho-alvo < ~800 linhas alteradas por PR.

### 3.1 Grafo

```
P1a tokens/estados/foco ──────┐
P1c atalhos (Shortcut.alt) ───┼─► P5 barra superior
U6 (outra sessão) ─► P1b ícones/glifos/figurinhas ─► P6 tabuleiro do painel ─► P7 motor
P1a + P1b ─► P2a lista (show_viewport, chips) ─► P2b marcas na página + clique com Mão
P3a1 store de correções + ferramentas ─► P3a2 cache por página ─► P3a3 correções nas leituras e exportações
P3b1 IllegalReason (crate chess)
P1a + P1b + P2a + P3a1 + P3b1 ─► P3b2 aba Diagrama + Confirmar ─► P3b3 rascunhos + modal ─► P3b4 Redesenhar (precisa de P3a2)
P3b2 ─► P3c teclado e leitor de tela no editor
P1c + P2a + P3b2 ─► P4 navegação
P8a PgnOrigin + etiquetas aditivas ─► P8b posições (unused, $220, skipped) ; P3a3 ─► P8b
P9a export::flow (Word/RTF) ∥ P9b HTML/EPUB acessíveis ; P2a + P3a3 ─► P9c menu, aviso de pendências, Abrir pasta ; P9d Markdown
P7 depende de P3b2
tudo ─► P10
```

**Podem começar já** (não tocam a interface nem os arquivos da U6): P1a, P1c, P3a1, P3b1, P8a, P9a, P9d.

### 3.2 Passos

Cada passo: **Contexto** (o que ler) · **Tarefas** · **Verificação** · **Saída** · **Reversão**. Em todos: entrada em `parity/`, ferramenta headless quando houver função nova, teste visto falhando antes da correção, i18n.

**P1a — Tokens, estados e foco** (`ui-egui/theme.rs`, `widgets.rs`, novo `chess_widgets.rs`). Tokens e `DiagramState` de §2.1; anel de foco nas pílulas. *Verificação:* teste de contraste. *Saída:* nenhuma cor de estado fora dos tokens. *Reversão:* revert.

**P1b — Ícones, glifos e figurinhas** (depois da U6). Caracteres soltos → ícones; `piece_sprites`/`piece_button` compartilhados (fundo casa clara); ícones novos (`cpu`, `file-output`, `badge-check`, `circle-dashed`, `eraser`, `circle-help`). *Verificação:* teste "nenhum glifo ausente em rótulo de botão". *Reversão:* revert.

**P1c — Atalhos por escopo** (`engine/commands.rs`, `canvas.rs`). `Shortcut.alt`; modificadores exatos; Alt+←/→ no histórico; teste de tabela sem colisão. *Saída:* Alt+→ não vira página (nota de versão). *Reversão:* revert.

**P2a — Lista de diagramas** (`chess_ui.rs`, depois da U6). §2.1 (cabeçalho, chips, linhas, cabeçalho de página, `show_viewport`, "Acompanhar a página"); FEN só na dica e no menu; ferramenta `chess_diagram_states` (contagens por estado). *Verificação:* 2000 diagramas sintéticos com quadro ≤ 4 ms (comando + máquina registrados no `blueprint.md` §8); testes de UI de filtro e menu por teclado. *Reversão:* revert.

**P2b — Marcas na página e clique com a Mão** (`chess_ui.rs`, `canvas.rs`). §2.1 "Marcas"; clique sem arrasto também com a Mão; "Marcar a página como conferida" (+ ferramenta). *Verificação:* testes de UI de clique com Selecionar e Mão; capturas. *Reversão:* revert.

**P3a1 — Store de correções** (`engine/src/chess/corrections.rs`, trait; implementações no `ui-egui` desktop/web e na CLI dentro de `--root`).
- Chave: `DocKeys` (`/ID[0]`, caminho, nome + tamanho — `engine/src/chess/keys.rs`, estável entre salvamentos) + por entrada: índice da página, **impressão digital da página**, retângulo normalizado na CropBox; casamento por **IoU ≥ 0,7**; páginas inseridas/removidas realocadas pela impressão digital.
- Esquema v1 em `<dados>/chess/corrections/<hash(DocKeys)>.json` (≤ 20 000 entradas, ≤ 4 MB; web com orçamento próprio, separado dos 4 MB do autosave):
  ```json
  {"format":"pdfcraft-chess-corrections","version":1,
   "doc":{"id0":"hex","name":"Livro.pdf","size":123,"pages":703},
   "entries":[{"uid":"…","page":16,"page_fp":"blake3:…","rect":[0.12,0.40,0.55,0.78],
     "orig":{"fen":"…","model":"key"},"fen":"<FEN completo>","kind":"verified|corrected",
     "at":"2026-10-21T12:00:00Z","rev":3,"deleted":false}]}
  ```
- Escrita: ler, mesclar e gravar sob trava de arquivo; por entrada vence o maior `rev`; tombstones; temporário + rename. Migração: versão maior → só leitura; menor → migrar; corrompido → `.bad`.
- Privacidade: sem texto nem miniatura do documento; caminho só como hash; ferramentas listam só o documento pedido.
- Ferramentas `chess_correction_list`, `chess_correction_set`, `chess_correction_delete` (um verbo cada, erros acionáveis).
- *Verificação:* testes de chave (caixa deslocada, página inserida, salvar e reabrir), de concorrência (duas escritas) e de migração. *Reversão:* só arquivos novos; um build revertido não os lê nem apaga.

**P3a2 — Cache de leituras por página** (`engine/src/chess.rs`). Edições registram `touched_pages`; após edição, mantêm-se as leituras das páginas não tocadas e relê-se só as tocadas; mudanças estruturais remapeiam. *Saída:* editar a pág. 5 de 703 relê 1 página (contador no teste). *Reversão:* revert (volta a reler tudo).

**P3a3 — Correções nas leituras e nas exportações** (`engine/chess.rs`, `ChessPage`, `export.rs`). A correção vence a releitura (divergência registrada na dica); `chess_document_with`/exportações e links usam a posição corrigida. *Verificação:* exportar depois de corrigir usa a correção; zero correções perdidas após editar/desfazer/detectar/salvar/fechar/reabrir. *Reversão:* revert.

**P3b1 — Motivo da ilegalidade** (crate `chess`). `IllegalReason` (rei faltando, dois reis, lado que não joga em xeque, peão na 1ª/8ª); `GameContext::from_fen` devolve o motivo; roque ajustado às peças. *Verificação:* testes unitários + wasm. *Reversão:* revert.

**P3b2 — Aba "Diagrama" com Confirmar** (`docking.rs`, `chess_editor.rs`, `chess_ui.rs`). `docking::Panel::DiagramEditor` com **migração do layout salvo**; definir onde a aba fica com o tabuleiro destacado (U5: vai junto para a janela de xadrez); layout de §2.1; "Confirmar e seguir"/"Só confirmar" gravando pelo P3a1; estados Conferido/Corrigido; editor antigo atrás de uma configuração por uma versão. *Verificação:* teste de UI do fluxo; capturas. *Reversão:* desligar a configuração.

**P3b3 — Rascunhos e modal.** Rascunho por diagrama; modal só ao fechar, exportar ou detectar por cima com rascunhos; faixa de primeira vez. *Verificação:* testes de UI.

**P3b4 — Redesenhar no PDF** (precisa de P3a2). Diálogo da 1ª vez; sem releitura do livro inteiro; ferramenta headless correspondente.

**P3c — Teclado e leitor de tela no editor.** §2.3. *Verificação:* corrigir e confirmar só com teclado; nomes AccessKit inspecionados.

**P4 — Navegação** (depois da U6). Contador que segue o filtro; fim da revisão "Revisão concluída: 40 de 40 conferidos"; abrir no tabuleiro grava histórico; "Voltar ao diagrama 3 (pág. 17)"; numeração única. *Verificação:* testes de ida e volta e de fim de fila.

**P5 — Barra superior** (`chrome.rs`). Zoom e página de §2.1; trilho medido; recolher o painel de ferramentas como oferta lembrada; Ctrl+Shift+N. *Verificação:* nenhuma interseção de retângulos do trilho a 705 pt.

**P6 — Tabuleiro do painel** (depois da U6). §2.1; abas Diagramas/Texto no estilo das abas de encaixe (antes de P2a ou depois dele — não em paralelo: ambos tocam `chess_ui.rs:294`).

**P7 — Motor a um clique.** `cpu` na aba Diagrama, na lista e no tabuleiro; analisa a posição editada; assistente sem motor; lembra a análise. *Verificação:* motor falso (script UCI mínimo).

**P8a — `PgnOrigin` e etiquetas aditivas** (`chess`, `chessvision/doc/pgn.rs`, `markdown.rs`, `chess_board_ui.rs`, `chess_collection.rs`, `automation/tools.rs`). §2.4 PGN, exceto posições. *Verificação:* golden, reimportação, equivalência, oráculo. *Reversão:* revert (etiquetas só adicionadas).

**P8b — Jogos de posição** (precisa de P3a3). `positions`, `$220`, `positions_skipped`.

**P9a — `export::flow` para Word/RTF.** §2.4 Word/RTF. *Saída:* DOCX abre sem reparo; golden do PDF→Word intocado.

**P9b — HTML/EPUB acessíveis.** §2.4 HTML/EPUB. *Saída:* XHTML bem-formado; `epubcheck` sem erros quando instalado; orçamento de tamanho medido.

**P9c — Menu, aviso de pendências e "Abrir pasta"** (precisa de P2a e P3a3).

**P9d — Markdown.** §2.4 Markdown.

**P10 — Textos, guia, aprendizado e medição.** §2.2 em todo o módulo; `docs/chess.md` (estados, cores, atalhos, "Revisar um livro em 5 passos"); dicas de primeira vez; roteiro de capturas e de **medição das jornadas** pelo canal de controle (saída registrada); teste com 3 usuários que não programam.

## 4. Invariantes após todo passo

`timeout` + `-j 2`: `cargo fmt --check`; `cargo clippy -p <crates> -j 2 --all-targets -- -D warnings`; `cargo test -p <crates> -j 2`; `cargo check -p pdfcraft-chess -p pdfcraft-chessvision --target wasm32-unknown-unknown`; `cargo xtask layers && cargo xtask assets && cargo xtask parity`; `cargo xtask deny` quando mudar dependência; testes de i18n, contraste e tabela de atalhos; teste novo visto falhando com a correção revertida; ferramenta headless e entrada em `parity/` para função nova; capturas antes/depois no PR (1366 × 768, claro e escuro).

## 5. Métricas de aceitação

| Jornada | Antes | Meta |
|---|---|---|
| a) Detectar e entender o que revisar | 3 cliques, sem legenda | 2 + "Revisar agora" |
| b) Corrigir e guardar | n + 4; altera o PDF e apaga a lista | n + 2; sobrevive a tudo |
| c) Revisar em sequência | n + 3 por diagrama; "Próximo" descarta | n + 1 (Ctrl+Enter); certo = 1 tecla |
| d) Analisar com motor | 3 (6+ sem motor) | 1 |
| e) Voltar ao diagrama | Ctrl+[ só se mudou de página | 1 |
| f) Exportar com origem | 3; página física em `Pdf*` | 3 + aviso + "Abrir pasta"; origem legível em qualquer programa |
| g) Zoom e página impressa | escondidos | visíveis |

E ainda: zero correções perdidas; 100 % das ações principais visíveis a 1366 × 768; nenhum glifo ausente; nenhum texto colorido abaixo de 4,5:1; trilho sem interseção; jornadas a)–g) possíveis só com teclado; lista de 2000 diagramas ≤ 4 ms/quadro; HTML de 548 diagramas ≤ 15 MB; EPUB sem erros no `epubcheck`; DOCX sem reparo; PGN reimportado sem perda (próprio leitor e python-chess); ≥ 90 % de sucesso sem ajuda no teste com 3 usuários e 5 tarefas.

## 6. Riscos e reversão

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Chave instável orfana correções | Alta | Crítico | `DocKeys` + impressão digital da página + IoU (P3a1) |
| Conflito com a U6 | Alta | Alto | P1b, P2a, P4, P6, P3b2 só depois do merge da U6 |
| `flow` afetar o PDF→Word | Média | Alto | Módulo novo; golden do caminho atual intocado |
| Downgrade esconder dados | Média | Alto | Versão maior só leitura |
| HTML/EPUB grande demais | Média | Médio | Orçamento medido; `<use>` |
| Mudança de Alt+→ surpreender | Média | Médio | P1c isolado, nota de versão |
| Fonte restrita embutida | Baixa | Alto | Checagem de `fsType` |
| Layout de encaixe salvo quebrar com a aba nova | Média | Médio | Migração no P3b2; teste com layout antigo |
| Nova aba confundir quem usa o editor antigo | Baixa | Médio | Configuração por uma versão; nota de versão |

Reversão: interface por `git revert`; P3a1 só cria arquivos (build revertido não os lê nem apaga); P3b2 atrás de configuração; P8a só adiciona etiquetas.

## 9. Revisões

### Revisão 1 — interface gráfica (2026-10-21)
Incorporado: A8-V1–V4, I1–I8, polimentos; tokens claro/escuro; especificação §2.1; o próprio plano corrigido (ícones no lugar de glifos; azul fora dos estados; zoom movido; sem rótulos no trilho do PGN; ícone + palavra; recolher painel só com aviso).

### Revisão 2 — usabilidade (2026-10-21)
Incorporado: A8-U1 (correções persistentes), A8-U2 (navegação pelo filtro, "Confirmar e seguir", fim da fila), A8-U3 (editor pelo teclado), A8-U4 (atalhos por escopo); estados Conferido/Corrigido/Rascunho; rascunho por diagrama; "Analisar" visível; motivo da ilegalidade; clique com a Mão; numeração única; avisos antes de alterar o arquivo e na exportação; textos §2.2; foco e anúncios §2.3; jornadas e métricas §5. Rejeitado: aceitar em massa os "a revisar".

### Revisão 3 — exportações e engenharia (2026-10-21)
Incorporado: A8-E1–E9; contrato de `PdfPage` preservado (só adição); `PgnOrigin` único com teste de equivalência; Event só em jogos de posição; comentário no idioma do livro; opção de posições e `$220`; acessibilidade do SVG, metadados e `page-list` do EPUB; `fsType`; `export::flow` para Word/RTF; Markdown; chave por `DocKeys` + impressão digital + IoU; trait de store respeitando wasm e `--root`; concorrência, migração, esquema e privacidade; cache por página como passo próprio; `show_viewport` em vez de `show_rows`; grafo corrigido (paralelismos falsos removidos, arestas faltantes, passos divididos, U6 como pré-requisito); critérios de saída por passo; riscos e reversão; portões completos.
