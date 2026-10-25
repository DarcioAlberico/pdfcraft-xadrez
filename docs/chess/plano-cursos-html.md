# Plano — Cursos de xadrez em HTML: o que aproveitar · v2

**Status:** v2 · 2026-10-25 · nenhum passo começado · decisões D13–D18 pendentes (§4).
**Origem:** três páginas HTML de cursos de xadrez salvas pelo usuário (pasta local `pasta-html-teste/`, **fora do repositório**: são conteúdo de terceiros, nunca entram em commit, fixture ou teste). Análise de 2026-10-25, só dos **dados** que as páginas carregam e do que elas mostram na tela (servidas em `127.0.0.1` e olhadas no navegador). O código JavaScript delas não foi lido para copiar e não será: o comportamento descrito aqui vem do que a página faz, e o formato vem dos dados.
**Repositório:** `DarcioAlberico/pdfcraft-xadrez` (Rust, egui), `main` `959ddf1f` (até #164 e #121).
**Público de cada passo:** uma sessão de agente que começa do zero; cada passo traz contexto, tarefas, verificação, saída e reversão.
**Nomes:** passos **HC0–HC10** (HTML de cursos); decisões **D13–D18**, depois de D1–D12.

---

## 0. Leia antes de qualquer passo

- **Regras da casa:** `AGENTS.md`/`CLAUDE.md`: sem pânico (todo HTML, JSON e SVG é entrada não confiável), camadas (`cargo xtask layers`), `chess`/`chessvision` compilam para wasm, **toda função nova tem ferramenta headless** e entrada em `parity/acrobat-features.toml`, ferramentas respeitam `--root`. Portões de `docs/chess/blueprint.md` §4.
- **Proveniência (o ponto que não se desfaz depois do merge):**
  - Os três arquivos são de terceiros. Testes usam **fixtures sintéticas escritas por nós** no mesmo formato (um curso de 2 lições, um visualizador com 2 partidas, uma lista de 3 puzzles), com lances e textos inventados. Nunca copie lances, comentários, nomes de lições, torneios, jogadores ou peças dos arquivos reais para o repositório, nem para este documento.
  - Nenhum código JavaScript das páginas é lido para ser traduzido. O formato dos dados (§1) é o contrato; o comportamento de treino (§2.3) é o que a página mostra.
  - Nenhuma peça, tabuleiro em imagem, som ou ícone dessas páginas entra no repositório (§2.6). A decisão D4 continua: peças vêm da máquina do usuário.
  - Nenhum nome de marca em nome de tema, rótulo ou ícone. "ChessBase" e "lichess" só aparecem como nome de formato ou de site em documentação, como já acontece com `[%cal]`.
- **Textos:** string nova no `tl!()` (inglês é a fonte) e em **todos** os catálogos `.tsv`, com o SHA-256 de cada um atualizado no `ATTRIBUTION.toml` (testes `covers_ui_literals`).
- **Ícones:** só Lucide do projeto. `graduation-cap` e `lightbulb` ainda **não** estão no `ATTRIBUTION.toml`: entram pelo `cargo xtask assets` no passo que os usar.
- **Coordenação com o plano A8** (`docs/chess/plano-analise-8.md`, passos ainda abertos): HC6 depende de P1a (tokens); HC9a/HC9b dependem de P1c (atalhos) e dividem `chess_ui.rs` com P2a; HC3 e HC9c dividem `chess_collection.rs`/`tools.rs` com P8a; HC8 e HC10 dividem `doc/html.rs` com P9b. Em cada par, quem chega depois parte do `main` com o outro já mergeado.
- **Arquivos contestados:** `chess_board_ui.rs`, `chess_ui.rs`, `chess_collection.rs` e os catálogos. Um passo de cada vez nesses arquivos.

## 1. O que há nos três arquivos

### 1.1 Visão geral

| Arquivo | Tamanho | O que é | Onde estão os dados |
|---|---|---|---|
| Curso de abertura | 1,57 MB | Curso exportado para uma página só, com a aparência de um site de cursos | `<script type="application/json" id="cl-data">` (807 KB), `id="cl-psets"` (217 KB, 6 conjuntos de peças em SVG), `id="cl-site-pieces"` (56 KB, peças e figurinhas do próprio site) |
| `TS29.html` | 309 KB | "PBN Course Viewer": visualizador de PGN com treino, sobre chessboard.js + chess.js | `const PGN_RAW = \`…\`` (PGN puro, 30 partidas), `const COURSE = {…}` (os mesmos jogos em JSON) e `PIECES` (um conjunto em base64 sem prefixo `data:`) |
| Puzzles da semana | 1,60 MB | Página de puzzles semanal (30 posições) com pontuação | `window.<nome>=[…]` com `puzzle.fen`, `start_color`, `answer` (o resto do código é ofuscado e foi ignorado) |

### 1.2 Curso (`cl-data`)

```
{ bid, name, author, cover,
  chapters: [ { title, lid, items: [ { oid, name, has, tactic } ] } ],
  lessons: { "<oid>": { oid, title, chapter, fen, color, result, tactic, info,
                        draw0: [desenho],
                        plies: [ { san, cb?: {d: [nó]}, ca?: {d: [nó]}, dr?: [desenho] } ] } } }
nó      = {t:"c", h:"<texto com <br>>"} | {t:"s", v:"<n>...<SAN>"} | {t:"v", c:[nó]}
desenho = {o:"a", s:<casa>, e:<casa>, c:<cor>} (seta) | {o:"c", s:<casa>, c:<cor>} (círculo) ; cor ∈ g r b y
```

- `plies` é a linha principal em SAN inglês; um elemento com `san: null` só carrega um comentário (`cb`), em geral antes do primeiro lance.
- `ca` é o que vem depois do lance: comentários (`c`) e variantes (`v`). **Uma variante depois do lance k é alternativa ao lance k** (depois de "2… lance A" vem "(2… lance B 3. …)"), e o número no primeiro lance da variante confirma o lance. Dentro de `v`, a regra vale recursivamente. Conferido nas 399 variantes de primeiro nível; as 124 aninhadas ficam para a medição de HC2b.
- O comentário é texto com `<br>` (4 440 vezes) como única marcação; nenhuma entidade HTML neste arquivo, mas o leitor decodifica as comuns. Há símbolos Unicode no texto (`♘`, `•`).
- Sinais colados ao SAN (`!`, `?`, `!?`, `?!`, `!!`, `+`, `⩲`, `±`, `=`) só aparecem nos lances de variante (356 vezes); a linha principal não tem nenhum.
- `color` é o lado que o aluno treina; `tactic`/`info` marcam lições de tática e só de leitura.
- **Números deste curso** (critério de aceite em HC2b, medido fora do CI): 1 capítulo, 252 lições, todas da posição inicial e do lado branco; **6 752** lances na linha principal (mais 19 elementos `san: null`); 523 variantes com 2 882 lances; profundidade máxima de variante 4; 3 007 comentários, o maior com 1 858 bytes; 2 457 desenhos (804 setas azuis, 664 setas verdes, 639 círculos vermelhos, 350 círculos verdes); `draw0` vazio.

### 1.3 Visualizador de PGN (`TS29.html`)

- `PGN_RAW` é PGN de verdade dentro de um *template literal* do JavaScript (escapes `\$`, `` \` ``, `\\`; BOM no início). 30 partidas, todas posições de tática com `[SetUp "1"]`/`[FEN]`. **O primeiro meio-lance é sempre do lado que não resolve** (19 FENs com as pretas a jogar, 11 com as brancas); o resto é a solução, com variantes.
- **`[%tqu "<idioma>","<pergunta>","","","<lance em coordenadas>","",<pontos>]`** no comentário desse primeiro meio-lance (30 de 30, sempre 7 campos): a pergunta de treino no formato ChessBase. O lance-solução é legal **depois** do meio-lance que carrega o comando, não na posição do FEN. O significado dos campos vazios **não** está confirmado.
- `[Annotator "Beauty=<x.y> Diff=<n>"]`: beleza e dificuldade do puzzle, num campo PGN padrão. Também `EventType`, `EventCountry`, `SourceTitle`, `Source`, `SourceQuality`, `PlyCount`, `EventDate` (`SourceTitle` já vem preenchido: P8a não pode sobrescrevê-lo).
- `COURSE` repete as partidas: `{chapters:[{title, variations:[{oid, title, white, black, whiteElo, …, game:{initial, color, data:[{move, col, san, isKey, pgnBranches:[{startFen, nodes:[{key:"S", val}]}]}]}}]}]}`. `isKey` marca o meio-lance a partir do qual a página conta o treino.
- Peças: a página **busca** cburnett, maestro, alpha, kosal e kiwen-suwi no site do lichess em tempo de execução (não estão no arquivo); só um conjunto "clássico" vem embutido, em base64 sem prefixo `data:`.
- Na tela: lista de capítulos com marca de visitado, barra com jogadores/Elo/data/torneio/local, notação com figurinhas e variantes coloridas entre parênteses, rei em xeque em vermelho, ponto de "quem joga" no canto, setas e círculos do usuário com botão direito, modo **MoveTrainer**, "Analisar no lichess", "Baixar PGN", 11 temas de casas, 6 opções de peças, movimento livre, promoção com escolha de peça, som.

### 1.4 Puzzles

- Cada item: `{puzzle:{id, fen, start_color, answer, author, quiz, image, info, daily_at, pivot}, answers, solved, percent}`; `answer` é *movetext* PGN com variantes, começando pelo lance do adversário (`1… <lance> 2. <solução> …`), e se repete em base64 no `answer` de fora. `author` traz o torneio. `start_color` sempre bate com o lado a jogar do FEN.
- O FEN é a posição **antes** do lance do adversário: a página joga esse lance sozinha e o aluno responde (a convenção dos puzzles do lichess).
- Campos ignorados na importação, e ditos no relatório: `pivot`, `quiz`, `daily_at`, `answers`, `solved`, `percent`, `image`, `info` (vazios ou do estado do site).
- **Números** (aceite em HC2c): 30 itens, **26 posições distintas** (os pares 15/18, 16/21, 17/23 e 25/26 são o mesmo puzzle); nenhuma posição em comum com o `TS29`.
- Na tela: "1 de 30" com setas, Dica, Aleatório, "Mostrar o padrão tático", "Começar de novo", quatro níveis de resultado no fim, tema claro/escuro.

### 1.5 O que o programa já tem (conferido no `main` `959ddf1f`)

| Recurso dos HTMLs | No PdfCraft | Onde |
|---|---|---|
| Setas e casas coloridas, `[%cal]`/`[%csl]` G R Y B | **Tem**; o modelo guarda os desenhos e os reescreve numa forma fixa (desenhos primeiro, casas antes de setas) | `chess/src/game/drawing.rs:15, 77`; `chess_board_ui.rs:139-143, 803` |
| Comentário antes do lance | **Tem** | `Game` `comment_before` |
| Variantes aninhadas, NAGs 1–146 e 238–255 | **Tem**, até `MAX_DEPTH = 32` | `chess/src/game.rs:37`; #48, #102 |
| Rei em xeque, último lance destacados | **Tem** | `chess_board_ui.rs:872-878` |
| Notação com figurinhas, painel de lances ao lado | **Tem** | #156 `Panel::Moves` |
| Lista de jogos do documento, abrir PGN com vários jogos | **Tem**, plana, **no máximo 256 jogos na lista inteira** (o mesmo limite vale para o autosave e para `chess_games`), PGN até 32 MiB | `engine/src/chess/collection.rs:25, 74`; `ui-egui/src/chess_autosave.rs:44` |
| Comentários | **Tem**, até `MAX_COMMENT_LEN = 4096`, **cortados em silêncio** em `set_comment` | `chess/src/game.rs:43, 708` |
| Peças do usuário: fonte de diagrama ou pasta `wK.svg`…`bP.png` | **Tem**, uma fonte por vez, em duas ordens de prioridade (`find` e `find_for_board`) | `chessvision/src/pieces.rs:15-23`, `pieces/images.rs` |
| Leitor de SVG das peças | **Tem, mas não está pronto para arquivo de terceiros:** usa `usvg::Options::default()`, cujo resolvedor de `href` lê arquivos do disco | `chessvision/src/pieces/svg.rs:17` |
| Link para analisar no lichess, exportar PGN | **Tem** | `chess/src/lichess.rs`, `chess_export_pgn` |
| Exportação HTML/EPUB com diagramas SVG | **Tem**, estática, **sem setas nem círculos** (`draw::board` não recebe desenhos) | `chess/src/draw.rs:235`, `svg.rs`, `chessvision/src/doc/html.rs` |
| Cores das casas | **Fixas** (marrom 240,217,181 / 181,136,99), repetidas fora de `draw.rs` | `draw.rs:196-207`; `chess_board_ui.rs:791, 855, 902, 919`; `chess_editor.rs:230` |
| Importar HTML de curso | **Não** | — |
| `[%tqu]`, `Annotator "Beauty/Diff"` | **Não lidos** (o comando fica no texto do comentário, sem perda) | — |
| Modo treino / quiz / progresso | **Não** | — |
| Escolher entre vários conjuntos de peças | **Não** | — |
| Marca na notação de que o lance tem desenhos; ir de variante em variante | **Não** (`chess_pgn_edit` tem voltar/avançar/escolher) | `automation/src/tools.rs` |

## 2. O que aproveitar, e como

### 2.1 Importar os três formatos (HC2a–c, HC3)

Um leitor por formato, todos dando `Vec<Game>` com etiquetas, para entrar na lista de jogos do documento como **Abrir PGN…** já faz (um passo de desfazer).

- **Curso (`cl-data`):** cada lição vira uma partida. Etiquetas: `Event` = nome do curso, `Annotator` = autor (quando houver), e as novas **`Chapter`**, **`Lesson`** (título) e **`TrainSide`** (`white`/`black`), só por adição (D16). `SetUp`/`FEN` quando `fen` não for a posição inicial. `<br>` vira quebra de linha, entidades comuns são decodificadas, qualquer outra marcação vira texto. Desenhos de `dr` vão para o lance; `draw0` para a posição inicial. Sinais colados ao SAN viram NAG pela tabela de #102; o que não for NAG conhecido fica no comentário.
- **Visualizador (`PGN_RAW`):** tirar os escapes do *template literal* e o BOM e ler com o leitor de PGN que já existe. O nome do capítulo vem de `COURSE.chapters[].title`, casado pela ordem; se `COURSE` faltar ou não casar, o PGN basta. Cada partida com `[%tqu]` no primeiro meio-lance ganha `TrainFrom "2"`.
- **Puzzles (lista JSON):** achar, em qualquer `<script>`, o primeiro *array* JSON cujos itens tenham `puzzle.fen` e `puzzle.answer` (sem depender do nome da variável, que é ofuscado). Cada puzzle vira `SetUp`/`FEN` + o *movetext* de `answer`; `Event` = `author`, a nova etiqueta **`PuzzleId`** = `id` (não `Round`), `TrainFrom "2"`. Duplicatas (mesmo FEN e mesma resposta) entram uma vez só, e o relatório diz quantas saíram.
- **Formato desconhecido:** erro que diz quais formatos o PdfCraft lê. Nunca "importar nada" em silêncio.
- **Limites, os reais:** arquivo ≤ 32 MiB (`MAX_PGN_FILE_BYTES`); profundidade ≤ 32 (`MAX_DEPTH`); comentário ≤ 4 096 bytes (`MAX_COMMENT_LEN`); lista ≤ 256 jogos (`MAX_GAMES`, a lista **inteira**). **Todo corte é contado e dito** no relatório (comentário cortado, variante funda demais, jogos que não couberam); o corte calado de `set_comment` passa a ser informado por quem importa. Um lance que não joga na posição para aquela linha ali, guarda o resto como comentário `{lances não lidos: …}` e marca a partida para revisão.
- **Mais de 256 jogos (HC3b):** um curso de 252 lições já não cabe numa lista com 5 jogos. Subir `MAX_GAMES` toca o formato do autosave, `MAX_BODY_BYTES` e o limite da versão web: é passo próprio.

### 2.2 Pergunta de treino `[%tqu]` e dificuldade (HC1)

- `chess` (L0): `TrainingQuestion { lang, text, solution: Option<Move>, points: Option<u16>, fields: Vec<String> }` como **vista só de leitura**: o comando continua no texto do comentário, intacto, e a função o lê de lá (`training_question(game, node)`). Os campos de significado desconhecido ficam em `fields`.
- A solução é conferida na posição **depois** do lance cujo comentário traz o comando.
- `Annotator "Beauty=… Diff=…"`: função que lê `beauty`/`difficulty` quando o texto tiver esse formato; a etiqueta não muda.
- Uso: o modo treino (HC4) mostra a pergunta e confere a solução; a lista de jogos pode ordenar e filtrar por dificuldade.

### 2.3 Modo treino (HC4a, HC4b, HC5)

O que as três páginas têm em comum, e que o PdfCraft não tem: **jogar a linha em vez de só lê-la**. Encaixa no que o programa já faz com livros: exercícios lidos com suas soluções (#50, #108, #128) viram treino sem esforço extra.

- **Quem treina e de onde**, nesta ordem: `TrainFrom` (o aluno começa nesse meio-lance; o lado é quem joga ali); senão, o meio-lance seguinte ao que traz `[%tqu]`; senão, `TrainSide` a partir do primeiro lance desse lado; senão, o lado que joga na posição inicial.
- **Fluxo:** o tabuleiro vira para o lado do aluno; os lances do adversário são jogados sozinhos (animados quando `motion::animated` deixar); o aluno joga o lance da linha principal arrastando, clicando ou digitando o SAN num campo (acessível pelo teclado). Lance certo: aparece o comentário do lance. Lance que é alternativa numa variante: "Também é bom, mas jogue o lance principal" e tenta de novo, sem contar erro. Lance errado: a peça volta, conta um erro, tenta de novo.
- **Dicas:** 1ª destaca a casa de saída; 2ª desenha a seta; 3ª joga o lance (conta como errado).
- **Aprender antes de treinar** (opcional): mostra a linha lance a lance com os comentários e depois pede para repetir.
- **Fim:** quatro níveis de resultado por erros e dicas; "Próximo", "Aleatório", "Começar de novo", "n de N" sobre a lista de jogos.
- **Progresso (HC5):** por partida, tentativas, último resultado e data da próxima revisão (caixas de Leitner: 1, 3, 7, 14, 30 dias; D14). Filtros "Não feitos", "Errados", "Para revisar hoje".
- **Headless:** a lógica fica no `engine`; a ferramenta `chess_train` recebe PGN + índice + lances tentados e devolve, por lance, `correct | alternative | wrong`, a dica de cada nível e o resultado. O `ui-egui` só desenha.

### 2.4 Temas de casas (HC6)

- `BoardTheme { id, light, dark, last_move, picked }`, com paletas **nossas e de nome genérico**: Marrom (o atual, padrão), Madeira clara, Azul, Verde, Cinza, Oliva, Roxo, Rosa. As cores passam no teste de contraste das coordenadas e do realce do último lance.
- Escolha em Preferências ▸ Xadrez e no menu `ellipsis` do painel do tabuleiro; vale para o tabuleiro, o de estudo e o editor (os pontos com cor fixa listados em §1.5 passam a ler o tema). As exportações têm opção própria (`board_theme` nas ferramentas `chess_export_*`), com **Marrom como padrão** nas ferramentas headless, para que os goldens não dependam da preferência.
- Fora: tabuleiros com textura em imagem até haver arte com licença aberta (D15). Coordenadas dentro das casas: não, o plano A8 §2.1 escolheu fora.

### 2.5 Conjuntos de peças (HC0, HC7a, HC7b)

- **Antes de tudo, HC0:** o leitor de SVG das peças passa a recusar toda referência externa (`href` de imagem, fonte ou arquivo) com um resolvedor próprio, e confere o tamanho em bytes antes de ler. Vale também para as pastas que o usuário já usa hoje.
- **Escolher entre vários (HC7a):** além de `$PDFCRAFT_CHESS_PIECES` (continua vencendo), procurar cada subpasta de `pieces/` ao lado do executável e cada fonte de diagrama em `fonts/`; mostrar a lista com miniaturas em Preferências ▸ Xadrez; guardar a escolha. As duas ordens de hoje (`find` para diagramas, `find_for_board` para o tabuleiro) continuam valendo quando nada foi escolhido.
- **Importar de um HTML (HC7b):** "Importar peças de um arquivo…" lê os conjuntos de 12 imagens SVG (`wK`…`bP`) que a página **traz dentro**, como `data:image/svg+xml` (o curso tem seis) ou base64 puro (o TS29 tem um), e grava cada um em `pieces/<nome>/wK.svg`… **na máquina do usuário**, nunca no repositório. Conjuntos que a página só busca na internet não são importáveis (o PdfCraft não acessa a rede). Antes de gravar, um aviso com a licença conhecida do conjunto (tabela abaixo) ou "licença desconhecida: só para uso pessoal".
- Licenças dos conjuntos citados nos arquivos, segundo o `COPYING.md` do lichess (lido em 2026-10-25):

| Conjunto | Onde aparece | Autor | Licença | Pode vir junto do PdfCraft? |
|---|---|---|---|---|
| cburnett | curso (dentro); TS29 (busca) | Colin M.L. Burnett | GPLv2+ | Não como está (o projeto não aceita GPL) |
| fresca, staunty, maestro | curso (dentro); maestro também TS29 (busca) | sadsnake1 | CC BY-NC-SA 4.0 | Não (não comercial) |
| california | curso (dentro) | Jerry S. | CC BY-NC-SA 4.0 | Não |
| chessnut | curso (dentro) | Alexis Luengas | Apache 2.0 | Sim, com atribuição (D13) |
| alpha | TS29 (busca) | Eric Bentzen | uso pessoal não comercial | Não; e não importável |
| kiwen-suwi | TS29 (busca) | neverRare | CC BY 4.0 | Sim, com atribuição (D13); não importável do arquivo |
| kosal | TS29 (busca) | não confirmado no `COPYING.md` | — | Não; não importável |
| "clássico" embutido do TS29 | TS29 (dentro) | desconhecido | desconhecida | Não; importar só com o aviso |
| peças e figurinhas do site (`cl-site-pieces`) | curso (dentro) | desconhecido | desconhecida | Não; importar só com o aviso |

### 2.6 O que **não** aproveitar

| Item | Por quê |
|---|---|
| Código JavaScript (chessboard.js, chess.js, o visualizador, o código ofuscado) | Regra da casa: só Rust; e o comportamento vem do que se vê, não da leitura de outra implementação |
| Logotipo, sprites, avatar, GIFs de resultado | Arte de terceiros sem licença aberta |
| Tabuleiro em imagem `webp` embutido no TS29 | Idem |
| Sons de lance e captura embutidos (os metadados citam um banco de sons comercial) | Som de terceiros; som próprio só se o usuário pedir, com arte aberta |
| Nomes de temas com marca | Marca; usamos nomes genéricos e cores nossas |
| Exportação HTML interativa com JavaScript | Regra da casa (sem JS escrito à mão); ver D17 |

### 2.7 Notação e leitura (HC8, HC9a–c, HC10)

- **Desenhos nos diagramas exportados (HC8):** `draw::board` recebe as setas e casas do lance; aparecem na substituição do diagrama no PDF, no SVG (HTML/EPUB) e no PNG (Word/RTF/Markdown), no mesmo estilo do painel (anel para casa, seta com ponta). Texto alternativo diz "seta verde de b5 a c7".
- **Notação (HC9a):** um ponto colorido depois do lance que tem desenhos (cor dos desenhos, com nome acessível "2 setas, 1 casa"); cabeçalho da partida com jogadores, Elo, torneio, data e local quando houver; modo de leitura "comentário do lance atual em destaque" para cursos com muito texto.
- **Variante anterior/próxima (HC9b):** botões e atalhos sem colisão (A8 P1c) e as mesmas operações em `chess_pgn_edit`.
- **Capítulos (HC9c):** a lista de jogos agrupa por `Chapter` quando a etiqueta existir, com contagem e progresso de treino por capítulo.
- **HTML estático do curso (HC10):** a exportação HTML/EPUB de uma lista de jogos (não só do livro reconhecido) com comentários, variantes e um diagrama em cada posição marcada (`$220`) ou com desenhos; sem script, como `svg.rs` exige.

## 3. Princípios

1. **Os dados são o contrato:** importar sem perder nada (comentário, variante, desenho, pergunta de treino); o que não se entende fica como texto e marcado para revisão; todo corte é dito.
2. **Contratos públicos só por adição** (etiquetas PGN novas, campos JSON novos nas ferramentas).
3. **Nada de terceiros no repositório**, nem em teste nem neste documento.
4. **A lógica fica embaixo** (`chess`, `chessvision`, `engine`), com ferramenta headless; o `ui-egui` desenha.
5. **Treinar não altera a partida:** o modo treino lê a partida e grava só o progresso.
6. Caber em 1366 × 768, teclado completo, cor nunca sozinha (A8 §2).

## 4. Decisões pendentes (do usuário)

| Id | Pergunta | Recomendação |
|---|---|---|
| **D13** | Vir junto do PdfCraft um conjunto de peças com licença aberta (chessnut, Apache 2.0, ou kiwen-suwi, CC BY 4.0), revendo D4? | **Não por enquanto:** manter D4 e entregar a escolha + a importação (HC7). Revisitar quando a fonte própria for decidida. |
| **D14** | Onde guardar o progresso de treino? | Junto do *autosave* da lista de jogos do documento, por partida, com campo novo opcional (HC5). Revisão por caixas de Leitner (1, 3, 7, 14, 30 dias). |
| **D15** | Tabuleiros com textura? | Não até haver arte aberta; só cores. |
| **D16** | Nomes das etiquetas novas | `Chapter`, `Lesson`, `TrainSide`, `TrainFrom`, `PuzzleId`; nenhuma etiqueta existente muda de sentido. |
| **D17** | Exportar curso como página interativa (com treino no navegador)? | **Não agora:** exigiria JavaScript (proibido) ou um visualizador em wasm, que é um projeto à parte. HC10 entrega o HTML estático. |
| **D18** | Vale escrever leitores para formatos de exportação de sites específicos (HC2b, HC2c), que podem mudar sem aviso? | **Sim, mas na ordem do valor:** HC2a (PGN embutido, formato aberto) primeiro; HC2b (curso) e HC2c (puzzles) depois, cada um tolerante e com medição própria. |

## 5. Passos

Branches `feat/chess-hc<n>-<slug>`. Tamanho-alvo < ~800 linhas alteradas por PR.

### 5.1 Grafo

```
HC0 SVG seguro (pieces/svg.rs) ─► HC7a escolha de peças ─► HC7b importar peças de HTML ◄─ HC2a
HC1 [%tqu] + Beauty/Diff (chess) ─► HC4a treino: lógica (engine) ─► HC4b treino: interface ─► HC5 progresso
HC2a split_pgn → chess; achador de <script>; leitor do PGN embutido
   ├─► HC2b leitor do curso JSON
   ├─► HC2c leitor da lista de puzzles
   └─► HC3a importar (chess_games path + Abrir…) ◄─ HC2b, HC2c ─► HC3b lista acima de 256 ─► HC9c capítulos
A8 P1a ─► HC6 temas de casas
HC8 desenhos nos diagramas exportados ─► HC10 HTML estático do curso ◄─ HC3a ; A8 P9b antes de HC10
A8 P1c ─► HC9a notação ─► HC9b variante anterior/próxima
```

**Podem começar já e em paralelo:** HC0, HC1, HC2a, HC8. **Contestados** (um de cada vez): HC3a, HC4b, HC6, HC9a–c.

### 5.2 Passos

Cada passo: **Contexto** · **Tarefas** · **Verificação** · **Saída** · **Reversão**. Em todos: entrada em `parity/` (com o que ainda falta na nota), ferramenta headless quando houver função nova, teste visto falhando com a correção revertida, i18n completo.

**HC0 — SVG de peças seguro** (`crates/chessvision/src/pieces/svg.rs`).
- *Contexto:* `glyph_from_svg` usa `usvg::Options::default()`; o resolvedor de `href` padrão do usvg lê arquivos do disco sem limite.
- *Tarefas:* `Options` com resolvedor que recusa todo `href` (imagem e fonte) e sem acesso a arquivos; limite de bytes antes de `Tree::from_data`; erro claro "o SVG aponta para outro arquivo".
- *Verificação:* SVG sintético com `<image href="C:/…">`, com `href` para arquivo inexistente enorme, com SVG aninhado em `data:` e com 10 MB: nenhum lê o disco (teste com caminho que, lido, falharia de forma visível), nenhum pânico; as pastas de peças sintéticas de hoje continuam lendo igual.
- *Saída:* nenhum SVG de peças lê arquivo além dele mesmo.
- *Reversão:* revert.

**HC1 — `[%tqu]` e dificuldade** (`crates/chess/src/game/training.rs`, novo).
- *Contexto:* `drawing.rs` (como um comando `[%…]` é achado no comentário); §1.3 e §2.2.
- *Tarefas:* `TrainingQuestion` e `training_question(game, node)` como vista só de leitura; a solução conferida na posição depois do lance; `annotator_scores("Beauty=… Diff=…")`. `chess_game` devolve `training` por lance.
- *Verificação:* PGN sintéticos com `[%tqu]` de 7 campos, campos vazios, aspas escapadas, comando malformado (ignorado, texto intacto), dois comandos no mesmo comentário, solução ilegal (`solution: None`, motivo dito); teste de propriedade (texto arbitrário nunca entra em pânico; ler → escrever → ler dá o mesmo `Game`).
- *Saída:* a pergunta e a solução de um puzzle sintético saem por `chess_game`.
- *Reversão:* revert.

**HC2a — Base dos leitores e PGN embutido** (`chess`: `split_pgn` movido de `engine/src/chess/collection.rs:405`, com reexportação no lugar antigo; `chessvision/src/course/` novo).
- *Contexto:* §1.3, §2.1; camadas (`chessvision` não pode chamar `engine`).
- *Tarefas:* mover `split_pgn`; `sniff(&str) -> Option<Format>`; `scripts(&str)` (acha blocos `<script>` sem parser HTML completo, com limites de quantidade e de bytes); `read_pgn_viewer` (escapes, BOM, capítulos de `COURSE`, `TrainFrom` pelo `[%tqu]`) → `CourseImport { games, chapters, duplicates, cuts, problems }`.
- *Verificação:* fixture sintética de 2 partidas nossas com `\$`, BOM, `[%tqu]`, `COURSE` ausente e `COURSE` que não casa; propriedade com bytes arbitrários (nunca pânico); `cargo xtask layers`.
- *Saída:* o TS29 do usuário importa 30 partidas com 30 `TrainFrom` (medição local `#[ignore]`).
- *Reversão:* revert.

**HC2b — Leitor do curso JSON** (`chessvision/src/course/lessons.rs`).
- *Tarefas:* `read_course_json`: lições, comentários antes/depois, regra da variante com o número como prova, desenhos, sinais → NAG, etiquetas de §2.1, cortes contados.
- *Verificação:* fixture sintética de 2 lições cobrindo `san: null`, variante de variante, `<br>`/entidades, as quatro cores de seta e círculo, FEN não inicial, lance ilegal no meio da variante, comentário acima de 4 096 bytes (cortado **e contado**), profundidade 33. Medição local `#[ignore]` (`PDFCRAFT_COURSE_SAMPLES=<pasta>`): 252 partidas, 6 752 lances principais, 523 variantes, 2 882 lances de variante, 2 457 desenhos, 0 cortes. Nenhum número do arquivo real em asserção do CI.
- *Reversão:* revert.

**HC2c — Leitor da lista de puzzles** (`chessvision/src/course/puzzles.rs`).
- *Tarefas:* achar o *array* pelos campos; `SetUp`/`FEN` + `answer`; `Event`, `PuzzleId`, `TrainFrom "2"`; duplicatas; campos ignorados listados no relatório.
- *Verificação:* fixture sintética de 3 puzzles nossos (um repetido, um com variante, um com `answer` ilegal). Medição local: 26 partidas, 4 duplicatas.
- *Reversão:* revert.

**HC3a — Importar: ferramenta e interface** (`automation` `chess_games`, `chess_collection.rs`; contestado com P8a).
- *Contexto:* `chess_games` já resolve `path` dentro de `--root` (`automation/src/chess_games.rs:55-62`); **Abrir PGN…** (#127: um passo de desfazer, aviso com contagem).
- *Tarefas:* `chess_games` aceita `.html`/`.htm` pelo mesmo `path` (em vez de ferramenta nova) e devolve o relatório de HC2; diálogo **Abrir PGN…** passa a aceitar HTML ("PGN ou curso em HTML"); aviso "252 partidas de 1 capítulo · 4 puzzles repetidos ignorados · 1 partida para revisar · 0 cortes"; quando a lista não tiver lugar, diz quantas entraram e quantas ficaram de fora.
- *Verificação:* confinamento (`--root`, caminho fora recusado; provado com a resolução desligada); teste de UI que abre a fixture e desfaz; contagens certas.
- *Saída:* numa lista vazia, o curso do usuário entra com as 252 lições; Desfazer as tira num passo.
- *Reversão:* revert.

**HC3b — Lista acima de 256 jogos** (`engine/src/chess/collection.rs`, `ui-egui/src/chess_autosave.rs`, versão web).
- *Tarefas:* novo `MAX_GAMES` (proposta 10 000) com a lista virtualizada; `MAX_BODY_BYTES` e o limite web revistos; autosave lê os dois tamanhos, sem mudar de versão se o formato não mudar.
- *Verificação:* lista sintética de 10 000 jogos: abrir, rolar com quadro ≤ 4 ms (comando e máquina registrados), salvar, reabrir; autosave antigo continua legível.
- *Reversão:* revert; listas maiores que 256 voltam a ser recusadas com aviso.

**HC4a — Treino: lógica** (`engine/src/chess/train.rs`, novo; ferramenta `chess_train`).
- *Contexto:* §2.3; HC1; `Game` e `rules`.
- *Tarefas:* `Session::new(game)` com lado e início pela ordem de §2.3; `try_move(Move) -> Verdict { Correct, Alternative, Wrong }`; `hint(level)`; `reply()`; `result() -> Grade` (4 níveis). Ferramenta `chess_train { pgn | path, game, moves[] }`.
- *Verificação:* testes de tabela com partidas sintéticas: puzzle com `TrainFrom`, puzzle só com `[%tqu]`, lição com `TrainSide "black"`, alternativa na variante, promoção, adversário com variantes (segue a principal), partida sem lances (erro claro), partida marcada para revisão (pula com aviso).
- *Saída:* uma sessão inteira jogável pela ferramenta, sem interface.
- *Reversão:* revert.

**HC4b — Treino: interface** (`chess_board_ui.rs`, `chess_ui.rs`; contestado; ícones novos `graduation-cap`, `lightbulb`).
- *Tarefas:* botão "Treinar" no painel do tabuleiro; faixa de estado ("Brancas jogam · lance 3 de 7 · 1 erro"); Dica com três níveis; campo de SAN; fim com resultado e "Próximo", "Aleatório", "Começar de novo"; Esc sai do treino sem mudar a partida.
- *Verificação:* testes kittest (certo, errado, dicas, fim; só teclado); captura a 1366 × 768 claro e escuro pelo canal de controle.
- *Reversão:* revert.

**HC5 — Progresso e revisão** (`ui-egui/src/chess_autosave.rs` e um *trait* de armazenamento no `engine`, no padrão do P3a1 de A8, para valer no wasm e sob `--root`; D14).
- *Tarefas:* `TrainRecord { attempts, last, due }` por partida, campo novo com `#[serde(default)]` (a `VERSION` não muda); filtros "Não feitos", "Errados", "Para revisar hoje"; "Treinar os exercícios deste capítulo" a partir dos puzzles lidos do livro; `chess_train` lê e grava o progresso num arquivo dentro de `--root`.
- *Verificação:* gravar, fechar, reabrir; relógio fixo nos testes; arquivo corrompido não perde a lista; autosave da versão atual sem o campo continua lendo.
- *Reversão:* revert; o campo extra é ignorado por versões antigas.

**HC6 — Temas de casas** (`chess/src/draw.rs`, `chess_board_ui.rs`, `chess_editor.rs`, `chess_study_ui.rs`, preferências; depois de A8 P1a; contestado).
- *Tarefas:* `BoardTheme` e as 8 paletas (§2.4); todos os pontos de cor fixa de §1.5 lendo o tema; preferência e menu do painel; `board_theme` nas exportações (padrão Marrom nas ferramentas headless); a preferência também pelo canal de controle (`set`).
- *Verificação:* contraste das coordenadas e do último lance em todas as paletas; golden SVG de um diagrama por tema; nenhum golden existente muda.
- *Reversão:* revert.

**HC7a — Escolher o conjunto de peças** (`chessvision/src/pieces.rs`, preferências; depois de HC0).
- *Tarefas:* `pieces::list()` com todas as fontes encontradas; preferência com miniaturas; `$PDFCRAFT_CHESS_PIECES` continua vencendo; ferramenta `chess_pieces` lista e escolhe (caminhos dentro de `--root`).
- *Verificação:* pastas sintéticas com SVGs nossos (formas geométricas), fonte Inter com papel forçado; escolha persiste; pasta com 11 peças é recusada com motivo.
- *Reversão:* revert.

**HC7b — Importar peças de um HTML** (depois de HC2a e HC7a).
- *Tarefas:* achar conjuntos de 12 SVGs embutidos (`data:image/svg+xml` com chaves `wK`…`bP`, ou base64 puro num objeto com essas chaves); aviso de licença (§2.5); gravar em `pieces/<nome>/` só depois do OK, sem sobrescrever sem perguntar; `chess_pieces { import: path, into: dir }` dentro de `--root`.
- *Verificação:* fixture sintética com 2 conjuntos geométricos nossos; SVG com `<script>` e `href` externo recusado (HC0); limites de tamanho; confinamento provado.
- *Reversão:* revert.

**HC8 — Desenhos nos diagramas exportados** (`chess/src/draw.rs`, `svg.rs`, exportações).
- *Tarefas:* `draw::board_with(placement, pieces, style, drawings)`; camadas de anel e seta; texto alternativo; diagramas das exportações de partidas recebem os desenhos do lance.
- *Verificação:* golden SVG e PDF com seta e anel; diagrama sem desenhos idêntico ao de hoje.
- *Reversão:* revert.

**HC9a — Notação** (`chess_ui.rs`; depois de A8 P1c e P2a; contestado).
- *Tarefas:* ponto de desenhos na notação; cabeçalho da partida; modo "comentário em destaque".
- *Verificação:* testes de UI; leitor de tela anuncia "2 setas"; captura.

**HC9b — Variante anterior/próxima** (`chess_ui.rs`, `automation/src/tools.rs`).
- *Tarefas:* botões e atalhos; `chess_pgn_edit` ganha `prev_variation`/`next_variation` ao lado de voltar/avançar/escolher.
- *Verificação:* tabela de atalhos sem colisão (A8 P1c); testes da ferramenta.

**HC9c — Capítulos na lista** (`chess_collection.rs`; depois de HC3a).
- *Tarefas:* grupos por `Chapter`, com contagem e progresso de treino (quando HC5 existir); `chess_games` devolve `chapter` por jogo.
- *Verificação:* testes de UI com a fixture do curso.

**HC10 — HTML estático do curso** (`chessvision/src/doc/html.rs` ou módulo irmão; depois de HC3a, HC8 e A8 P9b).
- *Tarefas:* exportar a lista de jogos (com capítulos) como HTML/EPUB: comentários, variantes, diagrama nas posições `$220` ou com desenhos; sem script.
- *Verificação:* XHTML bem-formado; golden com a fixture do curso; EPUB passa nas verificações de A8 §2.4.
- *Reversão:* revert.

## 6. Invariantes após todo passo

- Nenhum arquivo de `pasta-html-teste/` (ou derivado dele) no índice do git; nenhum lance, comentário ou nome tirado deles em fixture, teste ou documento.
- Ler → escrever → ler um PGN dá o mesmo `Game` (setas, `[%tqu]`, comentários); o texto pode mudar só onde já muda hoje (a forma fixa dos desenhos).
- Nenhuma ferramenta nova lê ou grava fora de `--root`; nenhum SVG lê outro arquivo.
- Gates da casa verdes nos crates tocados; `cargo test -p xtask` quando `parity/` mudar.

## 7. Riscos

| Risco | Mitigação |
|---|---|
| O formato dos sites muda | Leitor tolerante: campos desconhecidos ignorados e contados no relatório; `sniff` falha com mensagem, nunca importa pela metade calado (D18) |
| Variante ligada ao lance errado | Número do lance como prova + legalidade; se discordam, a variante entra como comentário e a partida vai para revisão |
| Cortes calados nos limites do modelo | Cada leitor conta comentários cortados, variantes fundas e jogos sem lugar; HC3a mostra |
| Licença de peças importadas | Aviso antes de gravar; nada vai para o repositório; D13 fica com o usuário |
| SVG de terceiros lendo o disco | HC0 antes de qualquer importação de peças |
| Arquivos contestados e passos abertos do A8 | Ordem do grafo (§5.1); cada passo parte do `main` atualizado |

## 8. Revisões

- **v1 (2026-10-25):** análise dos três HTMLs (dados e tela), comparação com o `main`, licenças das peças conferidas no `COPYING.md` do lichess.
- **v2 (2026-10-25):** revisão adversarial (código e dados medidos de novo). Corrigido: limites reais do modelo (256 jogos na lista inteira, 32 MiB, profundidade 32, comentário 4 096 cortado em silêncio) e o passo HC3b; 6 752 lances, não 6 771; quem treina nas partidas do TS29 (o primeiro meio-lance é do adversário; `[%tqu]` e `TrainFrom`); `[%tqu]` como vista só de leitura, porque os desenhos já são reescritos numa forma fixa; `split_pgn` para o `chess` (camadas); peças que o TS29 só busca na rede; o leitor de SVG lê arquivos do disco (HC0); conflitos com A8 (P1a, P1c, P2a, P8a, P9b); ferramentas headless para progresso, peças e navegação de variantes; HC5 no autosave do `ui-egui` com *trait*; HC2 e HC9 divididos; `PuzzleId` em vez de `Round`; dados de terceiros tirados dos exemplos; D18. Mantido: `motion::animated` existe (`motion.rs:147`).
