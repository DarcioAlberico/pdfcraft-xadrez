# Plano — Peças, tabuleiros e lista de lances (a partir de cursos em HTML) · v3

**Status:** v3 · 2026-10-25 · nenhum passo começado · decisões D13–D16 **aceitas** pelo usuário em 2026-10-25 (§4).
**Escopo (decisão do usuário, 2026-10-25):** melhorar o tabuleiro com **mais designs de tabuleiro e mais conjuntos de peças**, e **melhorar a lista de lances**. Fora: modo treino, perguntas `[%tqu]`, progresso, importação de cursos e puzzles como partidas, exportação de cursos (§2.5).
**Origem:** três páginas HTML de cursos de xadrez salvas pelo usuário (pasta local `pasta-html-teste/`, **fora do repositório**: são conteúdo de terceiros, nunca entram em commit, fixture, teste ou documento). Análise de 2026-10-25, só dos **dados** que as páginas carregam e do que elas mostram na tela (servidas em `127.0.0.1` e olhadas no navegador). O código JavaScript delas não foi lido para copiar e não será.
**Repositório:** `DarcioAlberico/pdfcraft-xadrez` (Rust, egui), `main` `959ddf1f`.
**Público de cada passo:** uma sessão de agente que começa do zero; cada passo traz contexto, tarefas, verificação, saída e reversão.
**Nomes:** passos **HC0–HC13**; decisões **D13–D16**, depois de D1–D12.

---

## 0. Leia antes de qualquer passo

- **Regras da casa:** `AGENTS.md`/`CLAUDE.md`: sem pânico (todo SVG e arquivo do usuário é entrada não confiável), camadas (`cargo xtask layers`), `chess`/`chessvision` compilam para wasm, **toda função nova tem ferramenta headless** e entrada em `parity/acrobat-features.toml`, ferramentas respeitam `--root`. Portões de `docs/chess/blueprint.md` §4.
- **Assets (`AGENTS.md` §1.2):** só MIT, Apache-2.0, BSD, ISC, CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, domínio público ou arte original de contribuidor. Cada arquivo com entrada no `ATTRIBUTION.toml` (SHA-256, fonte com commit, licença e arquivo de licença no repositório) e `cargo xtask assets` verde. Nada "só para uso pessoal", nada GPL/AGPL, nada não comercial.
- **Proveniência:** nenhuma peça, tabuleiro em imagem, som, ícone ou código das páginas analisadas entra no repositório. Peças de terceiros só pela fonte original e com licença da lista acima (§2.1). Tabuleiros com textura são **gerados por código nosso** (§2.2). Nenhum nome de marca em tema, conjunto ou rótulo.
- **Textos:** string nova no `tl!()` (inglês é a fonte) e em **todos** os catálogos `.tsv`, com o SHA-256 de cada um atualizado no `ATTRIBUTION.toml` (testes `covers_ui_literals`).
- **Ícones:** só Lucide do projeto; ícone novo pelo `ATTRIBUTION.toml` + `cargo xtask assets`.
- **Coordenação com o plano A8** (`docs/chess/plano-analise-8.md`, passos abertos):
  - **V3** (lista em árvore: recolher/expandir, recuo e barra por nível, trilha "onde estou", bifurcações, teclado) **já especifica** a estrutura da lista; esta spec **não repete** nada disso e acrescenta só o que V3 não tem (§2.3). HC9–HC13 tocam o mesmo `notation()`/`moves_column` que V3: um de cada vez, na ordem do grafo.
  - HC4 depende de P1a (tokens de cor); HC11 de P1c (atalhos); HC7 divide `doc/html.rs`/`svg.rs` com P9b.
- **Arquivos contestados:** `chess_board_ui.rs`, `chess_ui.rs`, `chess_editor.rs`, `chess_study_ui.rs` e os catálogos. Um passo de cada vez nesses arquivos.

## 1. O que as páginas mostram (só o que importa ao escopo)

### 1.1 Peças

- O curso de abertura traz **seis conjuntos SVG dentro da página** (`<script type="application/json" id="cl-psets">`: fresca, cburnett, staunty, maestro, california, chessnut), cada um com 12 imagens `wK`…`bP` em `data:image/svg+xml`, e o conjunto próprio do site (`cl-site-pieces`, com figurinhas `knight`, `queen`, `king`, `bishop`, `rook` usadas na notação).
- O `TS29.html` oferece seis opções; cinco (cburnett, maestro, alpha, kosal, kiwen-suwi) são **buscadas no site do lichess** em tempo de execução e uma ("clássico") vem embutida em base64.
- Ideia aproveitável: **muitos conjuntos à escolha, com prévia**, e **as figurinhas da notação desenhadas com o mesmo conjunto do tabuleiro**.

### 1.2 Tabuleiros

- `TS29.html`: 11 temas de casas, cada um com **cor clara, cor escura e cor de realce do último lance**, mais um tabuleiro com textura de madeira em imagem; coordenadas **dentro** das casas da borda, na cor da casa oposta; ponto de "quem joga" no canto; rei em xeque com fundo vermelho.
- O curso: casas marrons, setas grossas com ponta, **círculos como anéis** (não casa pintada), último lance em verde translúcido.
- Os puzzles: tabuleiro azul com leve textura, coordenadas pequenas dentro das casas.
- Ideia aproveitável: **temas com paleta completa** (casas + realces), **texturas**, **estilo das coordenadas**, **estilo dos desenhos**.

### 1.3 Lista de lances

- **Curso:** notação com **figurinhas**; o comentário do lance atual em destaque ao lado do tabuleiro (modo de leitura de curso); sinais `!`/`?` mostrados como **ícones coloridos** ("lance bom", "lance duvidoso"); comentário antes do primeiro lance; variantes aninhadas até 4 níveis. Estrutura dos dados: cada lance da linha principal tem comentário antes (`cb`) e depois (`ca`), e o "depois" alterna texto, lances e variantes; uma variante depois do lance k é alternativa ao lance k.
- **TS29:** barra de cabeçalho com **jogadores, Elo, data, torneio e local**; figurinhas; **variantes em outra cor**, entre parênteses, em bloco próprio; **resultado no fim**; botões **variante anterior / próxima**; uma **marca colorida no lance que tem setas** (com as cores das setas); clicar num lance de variante leva à posição.
- Ideia aproveitável: tudo o que está em negrito acima e não existe ainda (§1.4).

### 1.4 O que o programa já tem (conferido no `main` `959ddf1f`)

| Recurso | No PdfCraft | Onde |
|---|---|---|
| Peças do usuário: fonte de diagrama ou pasta `wK.svg`…`bP.png` | **Tem**, uma fonte por vez, em duas ordens de prioridade (`find` para diagramas, `find_for_board` para o tabuleiro); `PieceSet::geometric` como último recurso | `chessvision/src/pieces.rs:15-23`, `pieces/images.rs`, `chess/src/draw.rs:92` |
| Leitor de SVG das peças | **Tem, mas não pronto para arquivos de terceiros:** `usvg::Options::default()`, cujo resolvedor de `href` lê arquivos do disco | `chessvision/src/pieces/svg.rs:17` |
| Conjuntos de peças junto do programa | **Nenhum** (decisão D4: só da máquina do usuário) | `docs/chess/blueprint.md` D4 |
| Cores das casas | **Fixas** (marrom 240,217,181 / 181,136,99), repetidas fora de `draw.rs` | `draw.rs:196-207`; `chess_board_ui.rs:791, 855, 902, 919`; `chess_editor.rs:230` |
| Realces | Último lance, peça escolhida, destino, xeque: **cores fixas** | `chess_board_ui.rs:727-730, 872-878` |
| Setas e casas coloridas `[%cal]`/`[%csl]` | **Tem**; casa marcada desenhada como anel | `chess/src/game/drawing.rs`; `chess_board_ui.rs:139-143, 803` |
| Desenhos nos diagramas exportados | **Não** (`draw::board` não recebe desenhos) | `chess/src/draw.rs:235` |
| Coordenadas | Fora das casas (A8 §2.1), ligáveis em `BoardStyle.coordinates` | `draw.rs` |
| Lista de lances | Linhas por variante com recuo por nível, SAN **em letras** monoespaçadas, variantes menores e esmaecidas, comentário em itálico, parênteses, sinais, ícone de diagrama, lance atual selecionado e rolado à vista, menu do lance | `chess_board_ui.rs:1036-1188` (`notation`), `moves_column`, `moves_list` |
| Figurinhas | **Só ao copiar** ("Copiar linha com figurinhas") | `chess_board_ui.rs:1126` |
| Recolher variantes, trilha, bifurcações | **Não**, especificado em A8 V3 | `plano-analise-8.md` §7.3 |
| Cabeçalho da partida, resultado no fim, variante anterior/próxima, marca de desenhos, sinais como ícones, prévia ao passar o mouse, modo em colunas, tamanho da letra | **Não** | — |

## 2. O que fazer

### 2.1 Conjuntos de peças (HC0–HC3)

**Mais conjuntos no programa (HC1, D13).** Do `COPYING.md` do lichess (lido em 2026-10-25), estes têm licença que o projeto aceita:

| Conjunto | Autor | Licença | Estilo |
|---|---|---|---|
| chessnut | Alexis Luengas | Apache-2.0 | clássico arredondado |
| rhosgfx | RhosGFX | CC0-1.0 | clássico |
| fantasy | Maurizio Monge | MIT | ilustrado |
| spatial | Maurizio Monge | MIT | 3D estilizado |
| celtic | Maurizio Monge | MIT | ornamentado |
| kiwen-suwi | neverRare | CC-BY-4.0 | minimalista |
| firi | James Faure | CC-BY-4.0 | moderno |
| totoy | Kosal Sen | CC-BY-4.0 | moderno |
| papercut | Nikolay Anzarov | CC-BY-4.0 | recortado (o `COPYING.md` o lista entre os "não livres", apesar da licença: **conferir na fonte do autor** antes de entrar) |
| shapes | flugsio | CC-BY-SA-4.0 | formas geométricas |

- Não entram (licença fora da lista): cburnett, merida, mono, mpchess (GPL); letter, pirouetti, pixel (AGPL); fresca, staunty, maestro, california, caliente, cardinal, icpieces, gioco, tatiana, dubrovny, anarcandy, disguised, cooke, monarchy, horsey, minimal-warmth, xkcd (não comerciais); alpha, chess7, companion, leipzig, reillycraig, riohacha (uso pessoal, freeware ou sem licença); shahi-ivory-brown (sem derivados).
- Cada SVG vem da **fonte original do autor quando ela existir**, senão do repositório do lichess **num commit fixo**; licença conferida arquivo por arquivo; 12 entradas no `ATTRIBUTION.toml` por conjunto, arquivo de licença em `assets/chess/pieces/<conjunto>/LICENSE`, avisos no `NOTICE` (CC BY e Apache pedem atribuição).
- Os SVGs ficam em `assets/chess/pieces/<conjunto>/` e entram no binário (`include_bytes!`), lidos pelo mesmo caminho das pastas do usuário (depois de HC0). Peso esperado: dezenas a centenas de KB por conjunto; medir e registrar.
- Um **conjunto próprio nosso** (arte original, desenhada em código ou em SVG feito para o projeto) pode vir depois, como a "fonte própria" de D4; não é passo desta spec.

**Escolher o conjunto (HC2).** Preferências ▸ Xadrez ▸ Peças: grade de miniaturas (rei e cavalo de cada cor sobre casa clara e escura) com os conjuntos embutidos, as fontes de diagrama de `fonts/`, as subpastas de `pieces/` e a Merida instalada. Duas escolhas: **tabuleiro** (painel, estudo, editor, figurinhas da lista) e **diagramas** (redesenho no PDF e exportações), porque um livro impresso pede peças de livro. `$PDFCRAFT_CHESS_PIECES` continua vencendo; sem escolha, as ordens de hoje (`find`, `find_for_board`). Mostra a licença e o autor do conjunto escolhido.

**Importar peças (HC3).** "Importar peças…" aceita uma **pasta** (já lida hoje, agora copiada para `pieces/<nome>/`) ou um **arquivo HTML** que traga conjuntos SVG dentro (`data:image/svg+xml` ou base64 com chaves `wK`…`bP`). Grava **na máquina do usuário**, nunca no repositório, com aviso "licença desconhecida: uso pessoal" e sem sobrescrever sem perguntar. Conjuntos que a página só busca na internet não são importáveis.

**SVG seguro antes de tudo (HC0).** O leitor recusa toda referência externa (`href` de imagem, fonte ou arquivo) com um resolvedor próprio e confere o tamanho em bytes antes de ler. Vale para as pastas que o usuário já usa hoje.

### 2.2 Designs de tabuleiro (HC4–HC7)

**Temas com paleta completa (HC4).** `BoardTheme { id, light, dark, last_move, selected, target, check, coord_light, coord_dark, arrow_alpha }`, todos de **nome genérico e cores nossas**, com contraste conferido (coordenadas ≥ 4,5:1 sobre a casa; último lance distinguível nos dois tons de casa). Proposta inicial de 14: Marrom (o atual, padrão), Madeira clara, Nogueira, Azul, Azul-gelo, Verde, Verde-oliva, Cinza, Grafite, Roxo, Rosa, Areia, Coral, Alto contraste (acessibilidade). Valem para o tabuleiro, o de estudo, o editor e as miniaturas; os pontos de cor fixa de §1.4 passam a ler o tema.

**Texturas geradas por código (HC5, D14).** Nada de imagem de terceiros: cada textura é **função determinística** (ruído com semente fixa) que gera a casa clara e a escura em qualquer resolução, no `chess` (L0, sem IO) ou no `ui-egui`, com cache por tamanho. Propostas:
- **Madeira** (veios por ruído deformado, duas madeiras);
- **Mármore** (veios por turbulência);
- **Papel** (grão fino, para quem lê livros);
- **Impressão de livro** (casa escura **hachurada** em diagonal, como nos diagramas impressos: o estilo dos livros que o PdfCraft lê; também como estilo de **exportação** e de redesenho no PDF, em vetor).
Medir o custo: gerar 64 casas a 800 px de tabuleiro ≤ 30 ms uma vez, quadro seguinte sem custo (cache).

**Coordenadas, moldura e desenhos (HC6).**
- Coordenadas: **fora** (padrão, A8 §2.1), **dentro** das casas da borda na cor da casa oposta (como as páginas), ou **nenhuma**.
- Moldura: nenhuma, fina, dupla (de livro).
- Desenhos: espessura e opacidade das setas por tema; casa marcada como **anel** (hoje) ou **casa pintada** (opção).
- Ponto de "quem joga" no canto (opção; o "Quem joga" textual de A8 continua).

**Desenhos nos diagramas exportados (HC7).** `draw::board_with(placement, pieces, style, drawings)`: setas e anéis na substituição do diagrama no PDF, no SVG (HTML/EPUB) e no PNG (Word/RTF/Markdown), no estilo do tema; texto alternativo "seta verde de b5 a c7". As exportações ganham `board_theme` e `piece_set`, com **Marrom e a escolha "diagramas"** como padrão nas ferramentas headless (goldens estáveis).

### 2.3 Lista de lances (HC8–HC13)

Complementa A8 V3 (que faz recolher/expandir, recuo e barra por nível, trilha, bifurcações e teclado). Aqui:

- **Figurinhas na tela (HC8, D15).** Os lances mostram a peça como **figurinha desenhada com o conjunto do tabuleiro** (uma pequena imagem inline do tamanho da letra, como as páginas fazem), ou letras no idioma da interface (pt-BR R D T B C), ou letras inglesas. Nome acessível sempre por extenso ("cavalo f3"). Vale também para a lista ao lado da página (`Panel::Moves`) e a janela destacada.
- **Disposição e tamanho (HC9).** Dois modos: **texto corrido** (o atual, com os parágrafos de V3) e **colunas** (nº | brancas | pretas; um comentário ou variante quebra a tabela e ocupa a largura, e a tabela retoma com "12… "). Tamanho da letra da lista em três passos (pequeno, normal, grande), lembrado.
- **Cabeçalho e resultado (HC10).** Acima da lista: brancas e pretas com Elo, torneio, local, data e rodada quando houver (só os campos preenchidos, sem `?`), em uma ou duas linhas; um clique abre os detalhes da partida que já existem. No fim da linha principal, o **resultado** (`1-0`, `0-1`, `½-½`, `*`).
- **Navegar de variante em variante (HC11).** Botões e atalhos "Variante anterior / próxima" (na ordem de leitura, entrando e saindo de variantes), ao lado dos de A8 V3; as mesmas operações em `chess_pgn_edit` (`prev_variation`, `next_variation`).
- **Marcas no lance (HC12).**
  - Lance com desenhos: um ponto depois do lance com as cores das setas/casas (dividido quando há várias), nome acessível "2 setas, 1 casa"; passar o mouse mostra os desenhos no tabuleiro sem ir ao lance.
  - Sinais de lance (`!`, `!!`, `!?`, `?!`, `?`, `??`) como **chips coloridos com o próprio sinal** (verde, verde forte, azul, amarelo, laranja, vermelho), nunca só cor; avaliações (`±`, `⩲`…) continuam como hoje.
  - Comentário **antes** do lance com recuo e estilo distintos do comentário depois; quebras de linha do comentário respeitadas.
- **Prévia ao passar o mouse (HC13).** Sobre qualquer lance, sobretudo de variante, uma miniatura do tabuleiro **depois** do lance (com o tema e as peças escolhidos, último lance realçado), depois de 400 ms, sem mudar a posição mostrada; desligável. Pelo teclado, a mesma prévia com o foco no lance.
- **Comentário em destaque (HC12, opção).** Modo de leitura em que o comentário do lance atual aparece grande sob o tabuleiro (útil em partidas muito comentadas); a lista continua ao lado.

### 2.4 O que **não** aproveitar

| Item | Por quê |
|---|---|
| Código JavaScript das páginas | Regra da casa: só Rust; comportamento observado, não lido |
| Peças cburnett, merida e as demais da lista "não entram" (§2.1) | GPL, AGPL, não comercial ou uso pessoal |
| Peças e figurinhas próprias do site do curso, conjunto "clássico" embutido do TS29 | Licença desconhecida |
| Tabuleiros em imagem (madeira do TS29, imagens de tabuleiro do lichess, AGPL) | Arte de terceiros; texturas nossas por código (HC5) |
| Nomes de temas com marca | Marca; nomes genéricos e cores nossas |
| Sons de lance | Fora do escopo; os embutidos são de um banco comercial |

### 2.5 Fora do escopo desta spec (decisão do usuário, 2026-10-25)

Modo treino (jogar a linha, dicas, resultado, revisão espaçada), leitura da pergunta de treino `[%tqu]` e da dificuldade `Beauty/Diff`, importação de cursos, visualizadores de PGN e listas de puzzles como partidas, listas acima de 256 jogos, capítulos na lista de jogos e exportação de cursos. A análise de v1/v2 desses itens fica no histórico do PR #165, caso voltem.

## 3. Princípios

1. **Licença provável antes de bonito:** um conjunto ou textura só entra com a licença conferida na fonte e a entrada no `ATTRIBUTION.toml`.
2. **Um tema, todos os lugares:** tabuleiro, estudo, editor, miniaturas, prévia e exportações leem o mesmo `BoardTheme`; nada de cor fixa espalhada.
3. **Cor nunca sozinha** (sinais com o próprio símbolo, desenhos com nome acessível), contraste conferido em todo tema, claro e escuro.
4. **A lista não esconde dados:** figurinha, coluna ou prévia são formas de mostrar; o PGN não muda.
5. **Só por adição** nos contratos públicos (campos novos nas ferramentas, padrão igual ao de hoje).
6. Caber em 1366 × 768, teclado completo (A8 §2).

## 4. Decisões (aceitas pelo usuário em 2026-10-25, como recomendadas)

| Id | Pergunta | Decisão |
|---|---|---|
| **D13** | Rever D4 e trazer **junto do programa** os conjuntos de licença aberta de §2.1 (chessnut, rhosgfx, fantasy, spatial, celtic, kiwen-suwi, firi, totoy, papercut, shapes)? | ✅ **Sim** (revê D4): é o que dá "mais conjuntos" sem depender do que o usuário tem na máquina; todos dentro da política de assets. As peças do usuário continuam funcionando. |
| **D14** | Texturas de tabuleiro geradas por código (madeira, mármore, papel, impressão de livro)? | ✅ **Sim:** arte nossa, sem licença de terceiros, em qualquer resolução. |
| **D15** | Figurinhas na lista: padrão figurinhas do conjunto, letras pt-BR ou letras inglesas? | ✅ **Figurinhas do conjunto**, com as outras duas como opção. |
| **D16** | Tema padrão continua Marrom (tela e exportações)? | ✅ **Sim** na tela e nas ferramentas headless; o usuário escolhe outro nas preferências. |

## 5. Passos

Branches `feat/chess-hc<n>-<slug>`. Tamanho-alvo < ~800 linhas alteradas por PR.

### 5.1 Grafo

```
Peças:      HC0 SVG seguro ─► HC1 conjuntos abertos no programa (D13) ─► HC2 escolher o conjunto ─► HC3 importar peças
Tabuleiro:  A8 P1a ─► HC4 temas com paleta completa ─► HC5 texturas por código (D14)
                                                    └─► HC6 coordenadas, moldura, desenhos
            HC4 ─► HC7 desenhos e tema nos diagramas exportados ◄─ A8 P9b
Lista:      HC2 ─► HC8 figurinhas na tela (D15)
            A8 V3 ─► HC9 colunas e tamanho
            HC10 cabeçalho e resultado            (independente de V3; só o topo e o fim da lista)
            A8 P1c ─► HC11 variante anterior/próxima
            HC12 marcas no lance                  (depois de HC8 ou HC9, o que vier antes, no mesmo notation())
            HC4 + HC12 ─► HC13 prévia ao passar o mouse
```

**Podem começar já:** HC0, HC10; HC1 depois de HC0 (D13 aceita). **Contestados** (um de cada vez, a partir do `main` atualizado): HC4–HC6, HC8–HC13 e A8 V3.

### 5.2 Passos

Cada passo: **Contexto** · **Tarefas** · **Verificação** · **Saída** · **Reversão**. Em todos: entrada em `parity/` (com o que ainda falta na nota), ferramenta headless quando houver função nova, teste visto falhando com a correção revertida, i18n completo, capturas a 1366 × 768 claro e escuro para o que for visível.

**HC0 — SVG de peças seguro** (`crates/chessvision/src/pieces/svg.rs`).
- *Contexto:* `glyph_from_svg` usa `usvg::Options::default()`; o resolvedor padrão lê `href` do disco sem limite.
- *Tarefas:* `Options` com resolvedor que recusa todo `href` (imagem, fonte) e sem acesso a arquivos; limite de bytes antes de `Tree::from_data`; erro claro "o SVG aponta para outro arquivo".
- *Verificação:* SVGs sintéticos com `<image href="C:/…">`, `href` para arquivo inexistente, SVG aninhado em `data:`, 10 MB: nenhum lê o disco (provado com um caminho que, lido, falharia de forma visível), nenhum pânico; as pastas sintéticas de hoje leem igual.
- *Saída:* nenhum SVG de peças lê arquivo além dele mesmo. *Reversão:* revert.

**HC1 — Conjuntos abertos no programa** (`assets/chess/pieces/`, `ATTRIBUTION.toml`, `NOTICE`, `chessvision/src/pieces/bundled.rs`; depois de D13 e HC0).
- *Tarefas:* trazer os conjuntos de §2.1 da fonte (commit fixo), um por um, com licença conferida; `bundled::sets() -> &[(id, name, author, license, files)]`; `PieceSet` de cada um pelo mesmo leitor SVG; `cargo xtask assets --write`.
- *Verificação:* `cargo xtask assets` verde; teste que carrega os 10 × 12 SVGs e confere caixas não vazias; peso do binário antes/depois registrado; um conjunto com licença não permitida faz o `xtask` falhar (provado).
- *Saída:* `chess_pieces` (HC2) lista 10 conjuntos embutidos. *Reversão:* revert (só assets e um módulo).

**HC2 — Escolher o conjunto** (`chessvision/src/pieces.rs`, preferências do `ui-egui`, ferramenta `chess_pieces`).
- *Tarefas:* `pieces::list()` com embutidos, fontes, pastas e Merida instalada; duas escolhas (tabuleiro, diagramas), lembradas; grade de miniaturas com autor e licença; `chess_pieces { list | choose: { board?, diagrams? } }`; `$PDFCRAFT_CHESS_PIECES` continua vencendo.
- *Verificação:* pastas sintéticas com SVGs nossos; escolha persiste e é aplicada no painel, no estudo, no editor e numa exportação; pasta com 11 peças recusada com motivo; teste kittest da grade pelo teclado.
- *Reversão:* revert; volta às ordens de hoje.

**HC3 — Importar peças** (depois de HC2).
- *Tarefas:* "Importar peças…" para pasta ou HTML; achar blocos `<script>` com limites de quantidade e bytes, sem parser HTML completo; conjuntos de 12 SVGs embutidos; gravar em `pieces/<nome>/` com aviso e confirmação; `chess_pieces { import: path, into: dir }` dentro de `--root`.
- *Verificação:* fixture **sintética** (HTML nosso com 2 conjuntos geométricos nossos); SVG com `<script>`/`href` recusado (HC0); confinamento provado com a resolução desligada; bytes arbitrários nunca entram em pânico.
- *Reversão:* revert.

**HC4 — Temas com paleta completa** (`chess/src/draw.rs` ou `chess/src/theme.rs`, `chess_board_ui.rs`, `chess_editor.rs`, `chess_study_ui.rs`, preferências; depois de A8 P1a).
- *Tarefas:* `BoardTheme` e as 14 paletas (§2.2); todos os pontos de cor fixa de §1.4 lendo o tema; preferência com prévia e menu `ellipsis` no painel; `set board-theme` no canal de controle.
- *Verificação:* teste de contraste em todas as paletas (coordenadas, último lance, xeque, escolhido); golden SVG de um diagrama por tema; com Marrom, nenhum golden existente muda.
- *Reversão:* revert.

**HC5 — Texturas por código** (`chess` L0 para a geração, `ui-egui` para o cache de texturas; depois de HC4 e D14).
- *Tarefas:* madeira, mármore, papel e impressão de livro como temas com textura; geração determinística por semente; hachura em vetor para PDF/SVG; cache por tamanho no `ui-egui`.
- *Verificação:* golden de pixels com semente fixa (tolerância declarada); tempo de geração medido (meta de §2.2) e registrado; redimensionar não refaz a textura a cada quadro (contador de gerações no teste).
- *Reversão:* revert; os temas lisos ficam.

**HC6 — Coordenadas, moldura e desenhos** (`draw.rs`, `chess_board_ui.rs`; depois de HC4).
- *Tarefas:* coordenadas fora/dentro/nenhuma; moldura nenhuma/fina/dupla; espessura e opacidade das setas por tema; casa como anel ou pintada; ponto de "quem joga".
- *Verificação:* goldens por combinação principal; contraste das coordenadas "dentro" em todos os temas; testes de UI das opções.
- *Reversão:* revert.

**HC7 — Desenhos e tema nos diagramas exportados** (`chess/src/draw.rs`, `svg.rs`, exportações; depois de HC4 e A8 P9b).
- *Tarefas:* `draw::board_with(..., drawings)`; setas e anéis no PDF, SVG e PNG; texto alternativo dos desenhos; `board_theme` e `piece_set` nas ferramentas `chess_export_*` e `chess_replace_diagram` (padrão de D16).
- *Verificação:* golden SVG e PDF com seta e anel; diagrama sem desenhos e com o tema padrão idêntico ao de hoje.
- *Reversão:* revert.

**HC8 — Figurinhas na tela** (`chess_board_ui.rs` `notation`, `chess_ui.rs` lista ao lado; depois de HC2 e D15).
- *Tarefas:* figurinha inline do conjunto do tabuleiro (imagem do tamanho da fonte, alinhada à linha de base), letras pt-BR ou inglesas por preferência; nome acessível por extenso; mesma escolha em "Copiar linha".
- *Verificação:* teste kittest mede a altura da linha (figurinha não a aumenta); AccessKit diz "cavalo f3"; captura nos três modos.
- *Reversão:* revert; volta às letras.

**HC9 — Colunas e tamanho da letra** (`moves_column`; depois de A8 V3).
- *Tarefas:* modo colunas (nº | brancas | pretas, quebras por comentário/variante, retomada "12… "); tamanho em três passos; escolha lembrada; vale para o painel, a lista ao lado da página e a janela destacada.
- *Verificação:* jogo sintético de 10 000 nós com variantes: quadro ≤ 4 ms nos dois modos (só o visível); testes de UI das quebras.
- *Reversão:* revert.

**HC10 — Cabeçalho e resultado** (`chess_board_ui.rs` topo e fim da lista).
- *Tarefas:* cabeçalho com os campos preenchidos (sem `?`), clique abre os detalhes; resultado no fim da linha principal.
- *Verificação:* partida sintética com e sem campos; o cabeçalho cabe a 1366 × 768 em uma ou duas linhas; nome acessível.
- *Reversão:* revert.

**HC11 — Variante anterior/próxima** (`chess_board_ui.rs`, `automation/src/tools.rs`; depois de A8 P1c).
- *Tarefas:* botões e atalhos na tabela de P1c; `chess_pgn_edit` `prev_variation`/`next_variation`; ordem de leitura documentada.
- *Verificação:* tabela de atalhos sem colisão; testes da ferramenta em árvore com 4 níveis.
- *Reversão:* revert.

**HC12 — Marcas no lance e comentário em destaque** (`notation`; depois de HC8 ou HC9).
- *Tarefas:* ponto de desenhos com as cores e nome acessível; desenhos mostrados ao passar o mouse; sinais como chips coloridos com o símbolo; comentário antes distinto; quebras de linha; modo "comentário em destaque".
- *Verificação:* contraste dos chips em claro/escuro; leitor de tela; testes kittest.
- *Reversão:* revert.

**HC13 — Prévia ao passar o mouse** (`notation`, miniatura com `draw`; depois de HC4 e HC12).
- *Tarefas:* miniatura depois de 400 ms (ou com o foco pelo teclado), tema e peças escolhidos, último lance realçado; desligável; respeita movimento reduzido.
- *Verificação:* a posição mostrada no tabuleiro não muda; teste kittest com tempo simulado; custo da miniatura medido.
- *Reversão:* revert.

## 6. Invariantes após todo passo

- Nenhum arquivo de `pasta-html-teste/` (ou derivado dele) no índice do git, nem em fixture, teste ou documento.
- `cargo xtask assets` verde; todo SVG embutido com licença da lista de `AGENTS.md` §1.2.
- Nenhum SVG lê outro arquivo; nenhuma ferramenta lê ou grava fora de `--root`.
- Com o tema padrão e sem escolha de peças, diagramas e exportações idênticos aos de hoje.
- Gates da casa verdes nos crates tocados; `cargo test -p xtask` quando `parity/` mudar.

## 7. Riscos

| Risco | Mitigação |
|---|---|
| Licença de um conjunto diferente na fonte do que diz o `COPYING.md` do lichess | Conferir arquivo por arquivo na fonte do autor (papercut em especial); na dúvida, não entra |
| Binário maior | Medir em HC1; se pesar, embutir comprimido ou deixar parte como pacote opcional |
| Texturas lentas em tabuleiros grandes | Cache por tamanho; geração medida (HC5) |
| Conflito com A8 V3 na mesma função | Ordem do grafo; V3 primeiro, depois HC9–HC13 |
| Figurinha desalinhando a linha | Medida no teste de HC8 |

## 8. Revisões

- **v1 (2026-10-25):** análise dos três HTMLs (dados e tela), comparação com o `main`, licenças das peças conferidas no `COPYING.md` do lichess.
- **v2 (2026-10-25):** revisão adversarial (código e dados medidos de novo): limites reais do modelo, quem treina nas partidas do TS29, `[%tqu]` como vista só de leitura, camadas, leitor de SVG que lê o disco (HC0), conflitos com A8.
- **v3.1 (2026-10-25):** D13–D16 aceitas pelo usuário como recomendadas.
- **v3 (2026-10-25):** escopo reduzido pelo usuário a peças, tabuleiros e lista de lances; treino, `[%tqu]` e importação de cursos saem (§2.5). Novos: conjuntos abertos no programa (lista completa do `COPYING.md` filtrada pela política de assets), temas com paleta completa, texturas geradas por código, coordenadas/moldura/desenhos, figurinhas na tela, colunas, cabeçalho, variante anterior/próxima, marcas e prévia, sem repetir A8 V3. Passos renumerados HC0–HC13.
