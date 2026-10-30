# Plano de implantação — Carregar PGN do livro · v1

**Status:** v6 · 2026-10-30 · R0 (#229), R1 (#231), R2 (#233), R3 (#234), R4 (#235) e R5 feitos; R6–R7 a fazer.
**Origem:** pedido do usuário em 2026-10-29: "quando temos o PDF digital e o PGN dele, daria pra implementar uma função carregar PGN para aproveitar esse PGN?". Analisamos o par em `C:\Users\AMD\Documents\PDFCraft_Arquivos\PDF e PGN`: *Beat the French Defence with 3.Nc3* (Harikrishna; 748 páginas, gerado pelo calibre a partir de um curso Chessable) e o PGN do curso (120 partidas, "Chapter N / Game M").
**Repositório:** `DarcioAlberico/pdfcraft-xadrez`. Crates: `chess` (L0), `chessvision`, `engine` (L6), `automation`, `ui-egui`.
**Público de cada passo:** uma sessão de agente que começa do zero. Cada passo traz o contexto, as tarefas, a verificação e o critério de saída.

---

## 0. Leia antes de qualquer passo

- **Regras da casa:** `AGENTS.md`/`CLAUDE.md`.
  - Nada pode entrar em pânico.
  - Camadas: `cargo xtask layers`.
  - `chess`, `chessvision` e `engine` compilam para wasm, sem acesso direto a arquivo no engine.
  - Toda função nova ganha uma ferramenta headless e uma entrada em `parity/acrobat-features.toml`.
  - As ferramentas respeitam `--root`.
- **Fidelidade:** o texto impresso do livro nunca é trocado em silêncio. Um lance que só o PGN resolve fica marcado como vindo do PGN. Uma divergência vai para a revisão; o app não escolhe sozinho.
- **Sem PGN, nada muda:** cada passo deixa os 38 livros do `cargo xtask chess-bench` idênticos (25.571 válidos em 2026-10-29), porque o alinhamento só roda quando há um PGN.
- **Arquivos do usuário:** o livro e o PGN nunca entram no repositório nem nos testes. Os testes usam documentos e PGNs sintéticos. A medição no livro real é local (passo R7).
- **Textos:** toda string nova vai no `tl!()` (o inglês é a fonte) e nos 18 catálogos completos, com o SHA-256 atualizado em `ATTRIBUTION.toml`.

## 1. O que a análise encontrou

### 1.1 O par PDF + PGN

- **O PGN tem todos os lances e nenhum comentário.** Traz a linha principal, as variantes e os NAGs exatos (`$16`, `$1`, `$6`), escreve o roque com zero (`0-0`) e repete o lance antes de cada variante (`3.Nc3 3...g6`).
- **O PDF tem a explicação.** Traz a prosa do autor, os rótulos de linha (`A)`, `B1)`, `B2.1)`), variantes entre colchetes, diagramas com a legenda "Position after: 12.Nd6+!" e figurinas nos lances.
- **A ordem é a mesma.** As partidas do PGN seguem o livro: as páginas 16–22 são o Chapter 1/Game 4, as 23–32 o Game 5, as 33–41 o Game 6. A ligação entre página e partida, contada pelas posições que só uma partida tem, não deixa dúvida em nenhuma página de 8 a 64.

### 1.2 Medição (páginas 5–64, `chess_document` com `ocr: off`, app de 2026-10-29)

| | Resultado |
|---|---|
| Diagramas lidos cuja posição está no PGN | 73 de 73 (todos com `side_assumed`) |
| Lances válidos que caem numa posição do PGN | 1.000 de 1.036 |
| Lances não jogados (ilegal, não alcançado, fora de ordem, legenda) que o PGN resolve | 128 de 223 |
| Lances válidos em que o PGN aponta outro caminho | 23 (provável ramo errado do leitor) |

- **Simulação:** `cmp.py` e `sync2.py` no scratchpad da sessão b568d303. O leitor joga primeiro. Quando um lance não entra, o lance é procurado entre os filhos das últimas 60 posições vistas (lances e diagramas, em ordem de leitura), respeitando o número e o lado impressos.
- **Ganho esperado:** de 1.036 para cerca de 1.160 lances válidos de 1.259, ou seja, de 82% para cerca de 92%.
- **O que fica de fora:** sumário (`3...dxe4` sem contexto), lances citados na prosa e linhas que o PGN não tem (3.Nd2 na introdução).
- **Um caso típico:** na página 13, o leitor leu "Position after: 8.Nge2" como um lance novo, e a linha parou. O diagrama logo acima é exatamente a posição do PGN depois de 8.Nge2, e os sete lances seguintes (8...Bxb5 9.Nxb5 Nf6 10.Qf5 0-0 11.Bg5 …) são filhos dela no PGN.

### 1.3 Bug encontrado e corrigido (R0, #229)

`Game::play_san` aceitava só SAN canônico, e `0-0` encerrava a linha. 83 das 120 partidas paravam no primeiro roque no "Carregar PGN" do Tabuleiro e nas ferramentas `chess_game`, `chess_pgn_edit` e `chess_games`. Agora todas as 120 carregam inteiras.

## 2. Princípios de desenho

1. **O PGN é uma referência, não uma substituição.** O leitor de lances (`Walker`) continua lendo o livro como hoje. O alinhamento é uma etapa depois dele, sobre o `ChessDoc` inteiro. Assim o cache por página (`add_read_page`, P3a2) continua valendo, e um livro sem PGN não muda nada.
2. **Cada lance do livro tem uma ligação com o PGN, e a ligação diz de onde veio:**
   - `confirmed`: o leitor jogou e a posição bate com o PGN;
   - `recovered`: o leitor não jogou e o PGN resolveu;
   - `conflict`: o leitor jogou, mas o PGN aponta outro nó;
   - `absent`: o PGN não tem o lance.
3. **A posição manda; o texto se adapta.** Diagramas e lances válidos ancoram o alinhamento pela posição (as peças mais o lado a jogar). O SAN impresso e o número escolhem entre os filhos das âncoras próximas.
4. **Nada é aplicado sem rastro.** Um lance recuperado mantém o texto impresso e ganha uma marca de revisão. Um diagrama que difere do PGN gera uma sugestão de correção, e só a pessoa confirma.
5. **O PGN fica com o documento.** Uma vez carregado, ele é lembrado pelas chaves do documento (`DocKeys`, como as correções), e o livro reabre já ligado.

## 3. Passos

### 3.1 Grafo

```
R0 (#229) ─ R1 índice + alinhamento ─┬─ R2 engine + ferramenta ─┬─ R5 guardar com o documento ─ R6 interface
                                     │                          └─ R4 PGN comentado
                                     └─ R3 diagramas
R7 medição + docs + parity: depois de R2, atualizado a cada passo
```

R3 e R4 podem correr em paralelo depois de R2. R6 só começa depois de R5.

### 3.2 Passos

#### R1 — Índice do PGN e alinhamento (`chessvision`)

- **Contexto:**
  - Novo módulo `crates/chessvision/src/doc/reference.rs`.
  - Usa `pdfcraft_chess::game::Game::from_pgn_report` para cada partida.
  - Para cortar o arquivo em partidas, use o mesmo corte de `engine/src/chess/collection.rs`; mova o corte para `chess` se precisar.
  - O teto de 256 partidas de `collection::MAX_GAMES` é do Tabuleiro. Cursos Chessable passam de 256 linhas, então a referência tem teto próprio: 4.096 partidas e 32 MiB.
- **Tarefas:**
  1. `ReferenceIndex::from_pgn(text) -> Result<ReferenceIndex, ReferenceError>`, com:
     - os nós por posição (as peças mais o lado a jogar; os direitos de roque não entram na chave, porque o diagrama não os mostra);
     - os filhos por SAN normalizado, sem `+`, `#` e glifos;
     - o nome de cada partida (Event/Site/White/Black) e as linhas paradas (`PgnReport`).
  2. `align(doc: &ChessDoc, index: &ReferenceIndex) -> Alignment`. Em ordem de leitura:
     - **Âncoras:** diagramas lidos, lances válidos do leitor e a posição inicial.
     - **Busca:** para cada lance, os filhos das últimas K = 60 âncoras cujo SAN bate, filtrados pelo número e pelo lado impressos.
     - **Legendas:** um lance de legenda ("Position after: X") cujo nó leva à posição do diagrama logo antes é `confirmed`, e o diagrama ganha o lado, o número e os roques do nó.
     - **Iteração:** repete até não mudar mais (a simulação parou em 3 rodadas).
     - **Empate:** entre candidatos, ganha a partida que a página já usa (o mapa página → partida da §1.1); se ainda empatar, a âncora mais recente. Se o empate continuar, o lance fica `absent` com os candidatos listados; nunca se escolhe ao acaso.
  3. `Alignment` com: a ligação de cada lance (página, bloco, inline → partida, nó, tipo), o mapa página → partida e a contagem por tipo.
- **Verificação:**
  - Testes unitários com livros sintéticos de 2 a 3 páginas: legenda lida como lance, linha parada que o PGN retoma, transposição entre duas partidas, PGN de outro livro (quase nada ligado) e PGN vazio.
  - Teste de propriedade: `align` nunca entra em pânico com `ChessDoc` e PGN arbitrários.
  - Medição local no livro real (§1.2): pelo menos 120 dos 223 lances recuperados e nenhum lance válido do leitor perdido.
- **Critério de saída:** as contagens da §1.2 reproduzidas pelo código em Rust, sem a simulação em Python.

#### R2 — Engine e ferramenta headless

- **Tarefas:**
  1. `chess_document` ganha `reference_pgn`, um caminho confinado a `--root`.
  2. A resposta ganha `reference: {games, stopped_games, linked: {confirmed, recovered, conflict, absent}, pages: [{page, game}], warnings}`.
  3. O `include: json` traz em cada lance o campo opcional `reference: {game, node, kind}` (com `serde(default)`, para o formato antigo continuar lendo).
  4. Um lance `recovered` mantém o status do leitor e ganha a marca `ReviewFlag::FromReference { san }`. A sessão de leitura do app usa a mesma função.
  5. Aviso automático: se nas primeiras 20 páginas lidas menos de 30% dos lances ligam ao PGN, `warnings` diz "o PGN não parece ser deste livro".
- **Verificação:** teste de ferramenta com PDF e PGN sintéticos; confinamento a `--root` provado com o caminho de fato resolvido (lição do teste inútil citado em `CLAUDE.md`); `parity` com a entrada `chess.book-reference-pgn` como `partial`.

#### R3 — Diagramas confirmados pelo PGN

- **Tarefas:**
  1. Um diagrama cuja posição bate com um nó ligado ganha o lado a jogar, o número do lance e os roques do PGN, e perde o `side_assumed`. Ele aparece como "Confirmado pelo PGN" (novo `corrections::Status::Reference`) e sai da fila de revisão.
  2. Um diagrama a 1–2 casas de diferença de um nó ligado na mesma página ou nas vizinhas vira uma sugestão de correção no Diagrama: "O PGN do livro mostra: …". Ela só entra no armazenamento de correções quando a pessoa confirma (`kind: corrected`), nunca sozinha.
- **Por que importa:** neste livro os diagramas já saem certos. Num livro escaneado com PGN, o PGN vira o gabarito e a fila de revisão encolhe.
- **Verificação:** testes com uma leitura sintética de uma casa errada; prova de que a correção não é gravada sem confirmação.

#### R4 — Exportar o PGN comentado

- **Tarefas:**
  1. `markdown::pgn_export` ganha uma fonte de referência. A saída tem os lances, as variantes e os NAGs do PGN do livro, mais o texto do livro, entre um lance ligado e o próximo, como comentário depois do lance.
  2. No comentário, as figurinas viram letras. A legenda "Position after:" vira `$220`. Os rótulos `A)`/`B1)` ficam.
  3. As tags `PdfPage`/`PdfPageLabel` vêm de `PgnOrigin` (P8a) e `SourceTitle` do título.
  4. Linhas que só o livro tem entram como variante com o comentário `{só no livro}`, ou são deixadas de fora, conforme a decisão D2.
  5. `chess_export_pgn` ganha `reference: true`, e o menu Exportar ganha "PGN comentado (PGN do livro)".
- **Verificação:**
  - A saída relida por `Game::from_pgn_report` sem nenhuma linha parada.
  - Relida também por python-chess no teste local.
  - Nenhum lance do PGN original perdido (contagem de nós igual ou maior).

#### R5 — Guardar o PGN com o documento

- **Tarefas:**
  1. Armazenamento `chess-reference`, no mesmo molde de `corrections` (P3a1): um JSON por documento, nomeado pelo hash das `DocKeys`, com trava e renomeação. No desktop, uma pasta (`DirStore`); na web, a memória com orçamento (`MemStore`).
  2. Ele guarda uma cópia do texto do PGN (até 32 MiB), o hash, o nome original e a data, conforme a decisão D3.
  3. Ferramentas `chess_reference_set` (path), `chess_reference_status` e `chess_reference_clear`.
  4. `chess_document` usa o PGN guardado quando `reference_pgn` não vem (desligável com `reference: false`, como `corrections`).
  5. `doc_save` com outro nome reescreve as chaves (`rekey`).
- **Verificação:** os testes do molde das correções (lock, arquivo de versão mais nova, arquivo ruim posto de lado), e Salvar como mantendo a ligação.

#### R6 — Interface

- **Tarefas:**
  1. Xadrez ▸ **Carregar PGN do livro…** abre o seletor de arquivo (L7). Depois vem um resumo: "120 partidas · 1.160 lances ligados · 23 divergências para revisar". Também há **Remover PGN do livro**.
  2. Ao abrir um PDF que tem ao lado um `.pgn` com o mesmo nome e nenhum PGN guardado, o app oferece usá-lo (só no desktop, uma vez por documento).
  3. Aba Texto: o lance `recovered` aparece com um sublinhado próprio e a dica "Lance lido do PGN do livro"; o `conflict` entra na fila de revisão, com a escolha "Usar o lance do PGN / Manter o do livro".
  4. Lista de diagramas: o chip **Confirmado pelo PGN**.
  5. Tabuleiro: abrir um lance ligado abre a partida do PGN naquele nó, com todas as variantes. A lista de partidas mostra as do PGN com a página de cada uma.
- **Verificação:** kittest dos fluxos; captura de tela a 1366 × 768, claro e escuro; teclado e leitor de tela como em P3c.

#### R7 — Medição, documentação e parity

- **Tarefas:**
  1. `cargo xtask chess-bench --reference DIR`, com pares `livro.pdf` + `livro.pgn` em `DIR`. O relatório mostra confirmed, recovered, conflict e absent por livro e a taxa do leitor contra o gabarito, como o "gêmeo digital" (`docs/chess.md`).
  2. Pedir ao usuário mais pares (Chessable, Forward Chess).
  3. Atualizar `docs/chess.md` ("Usar o PGN do livro"), `parity/` e o ROADMAP.
- **Critério de saída:** pelo menos 3 livros medidos; a nota em `parity/` diz o que ainda falta.

### 3.3 Estado (2026-10-30)

- R0: feito (#229).
- R1: feito em #231 (`chessvision::doc::reference`; `examples/reference_bench.rs` mede um livro). No livro da §1.2:
  - 1.000 dos 1.036 lances válidos confirmados, nenhum perdido; 24 em conflito e 12 ausentes.
  - 132 dos 223 lances não jogados resolvidos: 118 recuperados e 14 confirmados (legendas lidas como lance).
  - 73 de 73 diagramas ligados a um nó; 54 páginas com partida; 2 rodadas.
  - Índice em 106 ms (120 partidas, 12.691 lances, 10.916 posições), alinhamento em 2,5 ms (release).
- R1, o que mudou em relação ao texto do passo:
  - **O número impresso ordena, não filtra.** Um PGN que começa de um FEN com `0 1` numeraria errado, e o lance se perderia. O lado impresso continua filtrando.
  - **Ordem dos candidatos:** número igual, depois a partida da página, depois a âncora mais recente; entre transposições, o nó da própria âncora, depois o primeiro do PGN.
  - **Não sobra empate para ficar `absent`:** uma âncora dá no máximo uma posição para um SAN, então duas âncoras nunca empatam. O que sobra é escolher entre partidas que passam pela mesma posição (transposição), e a regra acima decide.
  - **Legenda:** o primeiro lance depois de um diagrama cujo nó tem as peças do diagrama é `confirmed`, qualquer que seja o status do leitor.
  - **Títulos ficam de fora:** um lance em título nomeia uma linha e não a joga.
  - **O corte do PGN em partidas** (`split_pgn`) passou de `engine::chess::collection` para `chess::game`; `collection` o reexporta.
- R2: feito.
  - `reference::link(doc, index)` faz o alinhamento e guarda a ligação em cada lance (`MoveInline::reference`: `{kind, game, node}`, com `serde(default)`). Um lance `recovered` mantém o status do leitor e ganha `ReviewFlag::FromReference { san }`, com o SAN do PGN. Ligar de novo substitui a ligação anterior. A ferramenta usa essa função, e o app vai usar a mesma no R6.
  - `chess_document` ganha `reference_pgn`, confinado a `--root`. O arquivo é recusado acima de 32 MiB antes de ser lido, e um PGN sem lances é recusado antes da leitura das páginas.
  - A resposta ganha `reference: {pgn, games, moves, stopped_games, linked, diagrams, pages, warnings}`. Páginas e partidas contam a partir de 1; no JSON do documento, `reference.game` conta a partir de 0, como os outros índices do documento.
  - Aviso: menos de 30% dos lances das 20 primeiras páginas lidas no PGN ("the PGN does not look like this book's"); outro aviso quando o PGN passa dos tetos.
  - Paridade: `chess.book-reference-pgn`, `partial`.
- R2, o que mudou em relação ao texto do passo:
  - **A sessão de leitura do app ainda não chama `link`.** O app não tem onde escolher o PGN antes do R6. A função é uma só, e o R6 só a chama.
  - **`stopped_games` traz também as partidas que não puderam ser lidas** (`error`), além das paradas (`stopped_at`).
- R3: feito.
  - `link` também guarda em cada diagrama a posição do PGN (`DiagramBlock::reference`: `{kind, game, node, fen, changes, before}`).
  - **Confirmado:** o diagrama cujas peças são as do lance do PGN com que foi alinhado ganha o lado a jogar, os roques e o número do PGN, e perde o `side_assumed` (também quando o FEN lido já era igual ao do PGN). As marcas `SideAssumed`, `Illegal` e `SideConflict` saem. A leitura original fica em `before`, e ligar de novo parte dela.
  - **Sugerido:** um diagrama a 1 ou 2 casas de uma posição ligada a um lance na mesma página ou nas vizinhas ganha a sugestão, com as casas que diferem (`changes: [{square, from, to}]`). A leitura não muda.
  - Uma posição que a pessoa fixou (`DiagramSource::User`) nunca muda, nem ganha sugestão. Um diagrama cujo lado foi lido (não suposto) e é o outro não é confirmado.
  - `corrections::Status::Reference` ("reference") e `Status::with_reference`: uma leitura (com confiança ou com dúvida) cuja posição o PGN tem sai da fila de revisão. `chess_diagram_states` ganha `reference_pgn` e conta esse estado.
  - Nada é gravado nas correções: o teste de ponta a ponta confere que a pasta `chess-corrections` continua vazia.
- R3, o que mudou em relação ao texto do passo:
  - **O app ainda não mostra o chip "Confirmado pelo PGN"** nem aplica a sugestão: sem o PGN guardado (R5), o app não tem como saber dele. No painel, `Status::Reference` aparece como lido até o R6.
  - **As partidas não são refeitas** a partir do lado novo de um diagrama confirmado. O leitor joga antes do alinhamento, e o `SideFromMoveNumber` já corrige o lado pelo número do primeiro lance.
- R4: feito.
  - `reference::annotate::annotated_pgn(doc, index, book, style)` escreve as partidas do PGN, com todas as linhas e NAGs, e o texto do livro como comentário. O texto entre um lance ligado e o próximo vai depois do primeiro. O texto depois de um título, antes do primeiro lance de outra partida, vai antes do primeiro lance dela. Sem título no meio, fica com o lance anterior.
  - Figurinas viram letras inglesas. Um diagrama ligado põe `$220` no lance. A deixa da legenda, "Position after:" / "Posição depois de" / "Stellung nach" (as palavras do leitor, `after_cue`), sai quando o lance que vem depois repete o lance atual ou o de um diagrama; o resto do texto da legenda fica. Rótulos `A)`/`B1)` ficam.
  - D2: um lance válido do leitor que o PGN não tem entra como variante a partir da posição de onde o leitor o jogou (o lance atual ou um dos 60 recentes), e o primeiro lance da linha ganha o comentário "só no livro" no idioma do livro (`origin::only_in_the_book`, 18 idiomas).
  - Cada partida que o livro discute ganha `PdfPage`, `PdfPageLabel` e `SourceTitle` da primeira página que a discute. As que o livro não discute saem como estão.
  - `chess_export_pgn` ganha `reference_pgn` (confinado a `--root`). `examples/reference_bench.rs` ganha `--annotate=SAÍDA.pgn`.
  - No livro da §1.2: 120 partidas, 368 lances comentados, 7 lances só no livro, 73 marcas de diagrama, em 10 ms. O python-chess relê sem erro e com todas as sequências de lances do PGN. Sobram 4 deixas "Position after:" diante de lances que o PGN não tem.
- R4, o que mudou em relação ao texto do passo:
  - **A ferramenta recebe o caminho** (`reference_pgn`), não `reference: true`: o PGN guardado é o R5.
  - **O menu Exportar "PGN comentado"** fica para o R6, junto com a escolha do PGN no app.
  - **Variantes iguais repetidas no PGN original** viram uma só: o leitor do `chess` segue a resposta que já existe. Nenhuma sequência de lances se perde.
  - **Texto que a extração do PDF põe fora de ordem** vai para o comentário errado; o alinhamento não tem como saber.
- R5: feito.
  - `engine::chess::reference_store`: um JSON por documento (`{"format":"pdfcraft-chess-reference","version":1,"doc":{…},"pgn":{…}}`), com nome e busca iguais aos das correções (`corrections::file_name`, `DocRecord`). Guarda o texto do PGN (até 32 MiB), o hash, o tamanho, o nome do arquivo (nunca o caminho), a data e as contagens de partidas e lances. Um arquivo cujo texto não tem o hash guardado não é aceito.
  - Como nas correções: um arquivo de versão mais nova é lido e nunca escrito nem removido; um arquivo ruim vai para `….bad`, nunca é apagado. A pasta (`DirStore`) escreve sob trava, por um arquivo de preparo renomeado por cima, com os mesmos auxiliares das correções (agora `pub(super)` em `corrections`); a memória (`MemStore`) tem orçamento próprio.
  - Ferramentas `chess_reference_set` (`path`), `chess_reference_status` e `chess_reference_clear`, com `reference_store` confinado a `--root` (padrão `.pdfcraft/chess-reference`).
  - `chess_document`, `chess_diagram_states` e `chess_export_pgn` usam o PGN guardado quando não recebem `reference_pgn`, a não ser com `reference: false`. A resposta ganha `reference_kept` (o que a pasta tinha).
  - `doc_save` com outro nome leva o PGN para as chaves novas (`chess_reference: "kept for the saved file"`).
- R5, o que mudou em relação ao texto do passo:
  - **A busca entre os arquivos lê só o documento registrado** (`recorded`), sem conferir o hash de cada PGN; o arquivo escolhido é conferido inteiro ao ser lido.
  - **Remover apaga o arquivo.** As correções deixam lápide porque se fundem; o PGN guardado é um só e só a pessoa pede para removê-lo.
  - **`chess_diagram_states` e `chess_export_pgn` também usam o PGN guardado**, não só `chess_document`, para as três ferramentas que aceitam `reference_pgn` se comportarem igual.
  - **O app ainda não usa a pasta.** O "Salvar como" do app leva as correções, mas não o PGN; isso entra no R6, junto com a escolha do PGN.
- R6–R7: a fazer.

## 4. Decisões do usuário

- **D1. Divergência entre o leitor e o PGN.** Recomendação: na aba Texto prevalece o livro, marcado para revisão; no PGN comentado prevalece o PGN, que é a fonte dos lances.
- **D2. Linhas que só o livro tem.** Recomendação: entram no PGN comentado como variante com `{só no livro}`.
- **D3. Guardar cópia ou só o caminho do PGN.** Recomendação: cópia local, para a ligação sobreviver se o PGN mudar de pasta.

## 5. Riscos

- **PGN de outra edição ou de outro livro:** coberto pelo aviso de R2 e pela taxa de ligação no resumo.
- **Transposições:** a mesma posição em várias partidas. Resolvidas pelo mapa página → partida e pela âncora mais recente; entre partidas que passam pela mesma posição, ganha o nó da própria âncora (R1).
- **Livros com lances que o PGN não tem** (texto de apoio, partidas citadas): ficam como o leitor deixou, sem perda.
- **Desempenho:** índice de cerca de 11 mil posições neste livro; o alinhamento é linear no número de lances vezes K = 60. Meta: abaixo de 50 ms para 748 páginas.

## 9. Revisões

- **v1 (2026-10-29):** primeira versão, a partir da análise da §1 e da correção R0 (#229).
- **v2 (2026-10-29):** R1 feito; a §3.3 traz as medidas e o que mudou em relação ao passo.
- **v3 (2026-10-30):** R2 feito; a §3.3 diz o que a ferramenta devolve e o que ficou para o R6.
- **v4 (2026-10-30):** R3 feito; a §3.3 diz como um diagrama é confirmado ou sugerido e o que ficou para o R6.
- **v5 (2026-10-30):** R4 feito; a §3.3 diz onde cada texto do livro vai no PGN comentado e o que foi medido no livro.
- **v6 (2026-10-30):** R5 feito; a §3.3 diz o formato do PGN guardado, as ferramentas e o que ficou para o R6.
