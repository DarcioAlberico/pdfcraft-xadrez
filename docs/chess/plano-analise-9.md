# Plano A9 — Análise profunda do PdfCraft Xadrez: roadmap e spec · v11

**Status:** v11 · 2026-11-11 · síntese das oito análises (A1–A8); 10 de 10 revisões (§9). Pronto para executar a partir do Passo 0 (P2) e do Q1a.
**Base:** as evidências `caminho:linha` são de `cc2d484c` (2026-10-31, PRs até #249), o mesmo código do `dist/app` instalado e da cópia de leitura `C:/Python-Chess2/wt-a9`. O `main` está em `128564e0` (#254, 2026-11-11), que já corrige o tabuleiro que saía da aba Diagrama (parte do A6-05), a fonte de diagrama que o Chrome renomeia para `OTS-derived-font` (o caso confirmado do A2-06) e o motor numa pasta dentro de `engine/` (a parte do motor do A6-10); o `dist/app` não foi refeito depois dele. Para ler no `main`: `chess_ui.rs` +6 a partir da linha 166; `engine/src/chess/analysis.rs` +5 nas linhas 48–67 e +43 a partir da 74; `chess_diagram_tab.rs` +20 nas linhas 41–171 e +28 a partir da 175.
**Origem:** oito análises em paralelo, todas com a lente do ub-mode: A1 núcleo `chess` · A2 texto → partidas e exportações · A3 visão · A4 motor, automação e segurança · A5 código da interface · A6 app rodando (57 capturas a 1360 × 705, 1440 × 900 e 1920 × 1080, claro e escuro) · A7 testes, métricas e portões · A8 produto. Os relatórios completos, com cenário, confirmação e proposta de cada achado, estão em `docs/chess/analise-9/`: A1–A8, R1–R10, o registro de ações `log.md`, os roteiros do analista e do crítico (`brief.md`, `review-brief.md`) e o `coerencia.py`, que confere o roteiro (§3.1). Os roteiros de medida e os dados que eles usam ficam **fora** do repositório, em `C:/Python-Chess2/analise-9-local/<A|R>n/`, com o nome do relatório (`s*.json`, `mk_*.py`, `held_loss.py`, `splits.py`, as sondas de A1/A2, o gerador do modelo hostil da R7, a receita da página c06 do A3): muitos derivam dos livros do usuário, e nada derivado deles vai para o repositório. Cada passo que cita um roteiro também o reescreve como teste. Aqui, **CONFIRMADO** = reproduzido (teste temporário, CLI release, app pelo canal de controle ou leitura do caminho inteiro); **PLAUSÍVEL** = lido no código, sem reprodução.
**Repositório:** `DarcioAlberico/pdfcraft-xadrez` (Rust, egui). O fork **não tem CI**: os portões locais são o único portão (§4, passo Q1a).
**Público de cada passo:** uma sessão de agente que começa do zero. Cada passo tem uma ficha em §3.4 (onda · faixa · depende de · PRs · formato gravado) e traz Tarefas, Verificação e Esforço; Contexto, Saída e Reversão aparecem só quando diferem do padrão de §3.3.
**Nomes:** achados `A<n>-NN` (analistas) e `R<n>-NN` (críticos); passos `A9-<onda><n>` (Q portões, T tipos, G guarda dos dados, F PGN fiel, L leitura do livro, V visão, X exportações, I interface, N novas funcionalidades); decisões do usuário `D17`… (continua a série D1–D16 dos planos anteriores).

---

## Sumário executivo (5 minutos)

**O que foi feito.** Oito análises e dez revisões leram as ~97 mil linhas de xadrez do PdfCraft e acharam ≈ 150 problemas. Os que mais pesam para quem trabalha com livros: **3 arquivos que derrubam o programa** (peças SVG e um modelo hostis); **29 casos em que algo some ou sai errado sem aviso** — a correção que pula para o diagrama vizinho depois de apagar uma página, o PGN com BOM ou acentos Latin-1, a variante que vira linha principal, a coleção que para de gravar acima de 256 partidas, a exportação que grava por cima do PDF aberto, todo PDF criado pelo app com o mesmo `/ID` —; e **o livro relido do zero a cada exportação** (≈ 5 min no Harikrishna).

**O que vai acontecer, nesta ordem.**
1. **Passo 0:** feito em 2026-11-11 o 1º PR — este plano e os relatórios no repositório, as provas que usam os livros em `C:/Python-Chess2/analise-9-local/` (fora dele); falta o 2º, as notas "Missing" no `parity/`.
2. **Núcleo (≈ 16 PRs, ≈ 2 semanas):** portão de testes; os 3 travamentos; `/ID`; exportações que não sobrescrevem; BOM e Latin-1; correção que nunca cai noutro diagrama; coleção e lixeira sem perda; leitor de PGN sem cortes; o livro lido uma vez por sessão; corrigir no modo Ler. Fecha 18 dos 29 casos.
3. **Logo depois:** um armazenamento só para correções e PGN do livro, resultados por documento, o texto entre diagrama e lance, troca segura de peças — 24 de 29.
4. **Ondas 2–5 (≈ 9 semanas):** leitura do livro sem perder texto; livro inteiro em ≤ 1,5 min (hoje ~7); a interface numa coluna "Xadrez" (a página volta a ter ≥ 940 pt a 1366 × 768); teclado e leitor de tela (WCAG 2.2 AA); exportações acessíveis.
5. **Onda 6:** lance clicável no PDF, "esta posição no livro", mate em N, leitura guardada em disco, marcadores, exercícios.

**O que você decide.** Já, numa mensagem: **D17** (correções, PGN do livro e partidas na pasta de dados do app — sim), **D25** (manter as decisões ao trocar o PGN do livro — sim), **D19** (Copiar PGN em 79 colunas — sim), **D28** (refazer o `dist/app` ao fim de cada onda — sim). Nas ondas 1–2: 15 min no ChessBase (**D18**) e instalar o NVDA (**D26**). Na onda 6: D20–D23.

**O que muda para você.** Cada onda termina com o `dist/app` refeito (o anterior guardado e completo, numa instância separada), notas da versão em português e, no fim, a ajuda em PDF refeita (onda E). Um build antigo aberto junto não vê o que o novo gravou; as notas dizem o quê.

**Custo.** ≈ 120 PRs (≈ 10–13 semanas com três sessões em paralelo). Se parar depois do núcleo, o que mais machuca já está resolvido.

## 0. Leia antes de qualquer passo

- **Regras da casa:** `CLAUDE.md`/`AGENTS.md` — nunca entrar em pânico; camadas (`cargo xtask layers`); `chess`/`chessvision`/`engine`/`automation` compilam para wasm32; toda função nova com ferramenta headless e entrada em `parity/acrobat-features.toml`; ferramentas confinadas a `--root`; sem rede; sem GPL/AGPL; nunca descartar ou reparar dado em silêncio.
- **Ciclo de cada passo (ub-mode):** (1) reproduzir o achado num teste que falha pela razão esperada; (2) a menor mudança que ataca a causa, não um guarda; (3) ver o teste passar; (4) reverter a correção e ver o teste falhar de novo (restaurar com `cp`, nunca `mv`); (5) arrumar sem mudar o comportamento; (6) registrar no `parity/` o que ficou faltando. Um teste que passaria com todas as funções devolvendo vazio não conta.
- **Dados antes de lógica:** os passos de redesenho (§2) mudam primeiro a forma do dado (tipos, formato do arquivo, papel do bloco) e só depois a lógica.
- **Contratos públicos só por adição:** campos JSON das ferramentas, etiquetas PGN, formatos v1 dos stores. Mudança de sentido exige campo novo.
- **Textos:** string nova no `tl!()` (inglês é a fonte) e nos 18 catálogos completos (os outros quatro — `el`, `pl`, `te`, `zh-hant` — já não têm 34 textos de xadrez cada e mostram inglês; o I14 os completa), com o SHA-256 de cada `.tsv` atualizado no `ATTRIBUTION.toml`.
- **Tela de referência:** 1366 × 768 (≈ 1360 × 705 pt úteis), claro e escuro; depois 1920 × 1080.
- **Coordenação:** `ui-egui`, os catálogos e `crates/fonts` são os arquivos mais disputados; uma sessão por conjunto de arquivos; `CARGO_TARGET_DIR` próprio por sessão (nunca reaproveitar o de outra); `CARGO_INCREMENTAL=0` com disco curto.
- **Livros do usuário** (Harikrishna + PGN, *Chess Evolution*, os 38 de `C:\BackUp FX8300\PDF Chess`) só para medir localmente; nunca entram no repositório nem em testes.
- **Onde estão os dados de medida (locais, nunca no repositório):** `C:/Python-Chess2/cargo-targets/{pol.json, dv100.json, fi30.json, flores.json, …}`; `hk60.json` e `ce60.json` em `C:/Python-Chess2/analise-9-local/A2/book/` e `…/A2/ce/`; os roteiros de cada relatório em `C:/Python-Chess2/analise-9-local/<A|R>n/`; os livros em `C:/Users/AMD/Documents/PDFCraft_Arquivos/` e `C:/BackUp FX8300/PDF Chess`. Um passo que cita um desses arquivos o lê daqui; um passo que não o acha diz isso no PR, em vez de pular a medida.
- **Leia antes de um passo:** este §0, a ficha do passo em §3.4, os padrões de §3.3 e as evidências (Anexo A) dos achados que ele lista. O resto do §2 é consulta. **Nomes:** "`pdfcraft_chess`" ou "crate `chess`" é a camada L0; "A9-L0" é o passo da onda L. "Tecla F1/F6" é tecla; "F1/F6" sozinhos são passos. Ids de outros planos vêm com o plano ("R6e do `plano-pgn-do-livro.md`", "P5 do plano A8").

## 1. O que a análise encontrou

### 1.1 Números

| Medida | Valor | Fonte |
|---|---|---|
| Código de xadrez | ~97 mil linhas de Rust: `chess` 13,6 mil · `chessvision` 30,4 mil (`src`; +1,5 mil em `examples`) · `engine/chess` 13,1 mil · `automation/chess*` 5,3 mil (src 2,7 + testes 2,6) · `ui-egui/chess_*` 25,4 mil (34 arquivos) · testes de UI 8,9 mil | `git ls-files` + `wc -l` (R1) |
| Ferramentas headless de xadrez | 27 `chess_*` (de 202) | `pdfcraft-cli tools` |
| Entradas de xadrez no `parity/` | 20 (id com `chess`), todas `partial` | `parity/acrobat-features.toml` (R1) |
| Funções com complexidade ciclomática ≥ 10 | 347 (chessvision 130, ui-egui 111, chess 46, engine 36, automation 15, export 9); ≥ 20: 75; ≥ 40: 15 | `crapper --no-coverage` |
| Piores CC | `nag_name` 108 (tabela legítima) · `chess_pgn::apply` 74 · `text_tab` 73 · `Walker::play` 64 · `Automation::chess_export` 63 · `Game::from_pgn_report` 60 · `Session::chess_document_with` 53 | idem |
| Duplicação ≥ 0,8 | 49 pares; os que são a mesma decisão: `corrections.rs` × `reference_store.rs` (3 pares no `dryer` + 2 blocos por leitura), `chess_corrections_ui.rs` × `chess_reference_ui.rs` (4), três lexers de PGN, rótulo de status (2), tela → casa (4 lugares). Os de `enum → &str` são falsos positivos de forma. | `dryer --threshold 0.8`, A7-13 |
| Mutação (amostra) | `rules.rs` 40 de 42 mortos (95,2 %; sobrevivem 2 em `rules.rs:215`, reis em fileiras vizinhas) · `san::disambiguation` 7 de 7 | `mutator` (A7-07) |
| Testes do crate `chess` | 238, 63 s frio, 39 s após tocar um arquivo | A7 |
| Cobertura de linhas | não medida (`cargo llvm-cov` não instalado) | — |
| Detectar o livro todo (748 pp.) | ~7 min no app; na CLI release, detectar 3,0 min e detectar + ler 4,7 min, num núcleo só; *Chess Evolution* (1.481 pp.) ≈ 12 min | A6, R8 |

### 1.2 Achados

Os ≈ 150 achados — cada um com gravidade, evidência `caminho:linha` e o passo que o fecha — estão no **Anexo A**, por área. As contagens estão em §1.1; os 3 abortos e os 29 itens de perda, na métrica do §5; o que o núcleo fecha, em §3.0.

### 1.3 Causas-raiz (o que os ~150 achados têm em comum)

1. **A mesma decisão em vários lugares.** Dois stores por documento (motor e UI), três lexers e dois escritores de PGN, três regras de "mesma partida", três regras de zugzwang, quatro implementações de job, duas regras de posição, dois locais padrão de armazenamento, dois gravadores atômicos, quatro "tela → casa", duas notações de lance na UI. Quase todo bug de divergência nasce aqui (A4-07, A1-05, A1-11/A2-09, A1-07, A5-03, A5-12, A4-12, A6-09).
2. **Forma de dado com perda.** O FEN em texto não guarda "lado desconhecido" (A1-14); o `Block` não guarda "folio/cabeçalho corrente/texto de diagrama" (A2-04, A2-06); a correção não guarda as vizinhas da página (A4-01); o `/ID` de um documento novo não depende do documento (A4-03); `ChessState` guarda estado de documento em vagas globais (A5-10/A6-01).
3. **Tetos de memória aplicados como regra de dados, sem registro.** Comentário, etiquetas, NAGs, 4.096 e 256 partidas, 10.000 lances: cada corte deveria virar um aviso (`PgnWarning`, `ReviewFlag`) — o modelo certo já existe (`TooDeep`, `OrphanVariation`).
4. **Operações não idempotentes, leituras que escrevem e ações sem desfazer.** Recarregar o PGN apaga decisões; ler um store renomeia arquivos; pôr links duas vezes empilha; reler uma página conta duas vezes na cifra do livro (A3-04); confirmar/esquecer uma correção não tem desfazer.
5. **Trabalho caro no thread da interface.** Autosave de todas as partidas, impressões digitais do livro, índice do PGN do livro.
6. **Métrica e teste que medem a coisa errada.** O `walk_bench` conta "válidos" (um lance válido mas errado conta como acerto); a aba Texto só é testada com varredura síncrona; testes com modelos se pulam sem aviso; o canal de controle não chega ao teclado do xadrez.
7. **Leiaute que mede depois de desenhar.** Tamanhos tirados de `available_width()` depois de um conteúdo que alarga (A6-05), rótulos sem truncar (A6-02, A6-13), barra de rolagem por cima (A6-15).
8. **Entrada hostil sem teto na dimensão que importa.** Conta-se o número de nós do SVG, não a profundidade (A3-01); conta-se todo ponto de tinta como glifo (A3-02); aceita-se fonte pelo nome do arquivo e não pelo conteúdo (A3-05); laços quadráticos dentro de tetos de quantidade (A3-08).

## 2. Spec — o que construir, a partir dos primeiros princípios

### 2.1 Glossário (uma palavra, um conceito)

| Termo (pt-BR na interface) | Sentido | Tipo |
|---|---|---|
| Documento | Um PDF aberto; identificado por `DocKeys` (`/ID[0]`, caminho, nome + tamanho) | `engine::chess::DocKeys` |
| Página (física) · rótulo | Índice 0-based no PDF · número impresso (`/PageLabels`, senão folio lido, senão índice + 1) | (novo, T2) `PageLabel { printed: Option<String>, physical: u32 }` por `page_label(doc, index)` |
| Conjunto de páginas | Páginas pedidas, ordenadas e sem repetição | (novo, G7) `PageSet` |
| Chave da página (cache) | `u128` sobre tudo o que muda o desenho da página; segue a página no cache e no `follow_pages` | `page_key` (`page_key.rs:35`) |
| Impressão digital da correção | `fnv:` sobre os fluxos de conteúdo e XObjects; gravada em `Correction.page_fp` | `page_fingerprint` (`corrections.rs:556`) |
| Diagrama | Região da página com um tabuleiro, numerada por página | `DiagramBlock` |
| Varredura (da página) | Os diagramas achados e lidos numa página | `engine::chess::ChessPage` |
| Texto lido (da página) | Blocos, lances e diagramas de uma página, como lidos | `chessvision::doc::ChessPage` |
| Leitura (do diagrama) | O que o modelo/fonte leu: posição, lado, orientação, dúvidas | `DiagramReading` (L6: `fen: Fen`, `side: SideToMove`, `orientation`) |
| Posição | Colocação + lado + roques + en passant + contadores, validada uma vez | (novo, T1) `Fen` |
| "Mesma colocação e vez" | Colocação + lado a jogar — a chave do livro (N3, alinhamento com o PGN do livro); com lado suposto, só a colocação (`has_position(fen, side_assumed)`, `reference.rs:256-262`). **Não** é a "mesma posição" da FIDE (art. 9.2.3: roques e en passant contam), que vale para repetição e para o cache do motor (`Fen::full()`) | (novo, T1) `BookKey` |
| Enunciado | O que o livro pede do diagrama: objetivo (mate em n, ajudamate, automate, ganhar, empatar), lado e alcance (diagrama, página, seção) | (novo, L6) `Stipulation { goal, side, scope }` |
| Solução múltipla · dual | Mais de uma chave num "#n" (o problema está demolido; num livro, sinal de leitura errada) · continuação alternativa depois de uma defesa | termos do N4 |
| Lado a jogar · lado suposto | Quem joga e de onde veio (impresso, legenda, número do lance, suposto) | (novo, L6) `SideToMove { color, source }` |
| Orientação | De que lado o diagrama está impresso | (novo, L6) campo de `DiagramReading` |
| Correção | Posição confirmada pelo usuário: **Conferido** (= leitura) ou **Corrigido** (≠ leitura) | `corrections::Entry` |
| Rascunho | Correção ainda não confirmada, por diagrama, na sessão | `chess_drafts` |
| Posição efetiva | Correção atribuída ao diagrama, se houver; senão leitura | `Corrected.applied` do motor (regra única `assign`, §2.4) |
| Detectar diagramas | A ação de achar e ler os diagramas (um só termo; "Procurar"/"Scan" saem) | `chess.scan` |
| Lance lido | O que foi lido de um lance (número, texto, `san_read`, negrito, retângulo, bandeiras do leitor) — sem nada do walk | `MoveInline` (A9-L0) |
| Lance percorrido | O que o walk decidiu para um lance (status, SAN, FEN, nó, bandeiras) | (novo, A9-L0) item de `Walk` |
| Endereço de lance | Página, bloco e inline de um lance lido | (novo, T2) um tipo só no lugar de `MoveAt`, `InlineAt` e dos campos de `Decision` |
| Endereço de diagrama | Página e índice de um diagrama | (novo, T2) `DiagramAt { page, index }` (com `DocId` na UI) |
| Partida (lida) · partida (tabuleiro) | A partida montada pelo walk · a árvore de lances editável | `doc::GameScore` · `pdfcraft_chess::Game`; `to_game` (`games/board.rs:128`) é a única ponte |
| Cifra do livro | O que o OCR aprendeu dos símbolos do livro, por página | `BookCipher` (`StoreKind` "cipher") |
| Origem do livro | Se o livro é escaneado ou nascido digital | `BookOrigin` |
| Linha principal · variante | Filho 0 · demais filhos de um nó | — |
| Comentário antes/depois | Texto ligado ao nó antes/depois do lance | `comment_before`/`comment` |
| Sinal | Símbolo de avaliação (NAG); "glifo" fica para o reconhecimento de caracteres | `Nag` |
| PGN do livro | PGN da editora, alinhado ao PDF pela posição | `reference` |
| Ligação | confirmed · recovered · conflict · absent | `MoveInline::reference` |
| Decisão | Escolha do usuário num conflito livro × PGN | `reference_store::Decision` |
| Coleção | Partidas do documento no tabuleiro, com autosave | `StoreKind` "games" (o `GameRecord` em tipos de L0/L6; a UI só converte) |
| Lixeira | Partidas como estavam antes de perder lances | `chess_trash` |
| Store | Arquivo por documento e por tipo, com trava, escrita atômica e regra de mescla | (novo) `DocStore<K: StoreKind, B: StoreBackend>` |
| Pastas do app | Instalação, dados e modo portátil | (novo, T3) `AppDirs`, resolvido só em L7 |
| Job | Trabalho em segundo plano com resultado, guarda e cancelamento | (novo) `Job<T>` |
| Aviso | Registro de um corte, reparo ou descarte feito pela leitura | `PgnWarning`, `ReviewFlag` |

O glossário vira teste (I14): as palavras proibidas ("Procurar diagramas", "glifo" para NAG, "Scan" no inglês da ação) não aparecem nos catálogos nem no texto-fonte inglês (`engine/src/commands.rs:378`, `catalog.rs:289`).

### 2.2 Identidade do documento (G1)

- `cos::ensure_id`: para um documento sem `/ID`, o hash cobre **todos** os bytes escritos (corpo serializado antes do trailer) — não os do arquivo de origem nem a amostra de 1 byte a cada `len/4096` de hoje (`writer.rs:568`), que deixaria colidir corpos diferentes fora da amostra; é linear e barato; mesma entrada → mesmo `/ID` (determinismo dos testes).
- `DocKeys::matches`: quando os dois lados têm `id0` e eles diferem, não há casamento (o `id0` **veta**). O `/ID` constante já gravado (`9BA0D5AAD0617164A1D073B0D8C7DCD9`) é tratado como ausente (`DocKeys::KNOWN_CONSTANT_ID`), para os arquivos que o usuário já criou; os arquivos de store já chaveados por ele continuam lidos pelo caminho.
- EPUB: `dc:identifier` inclui o intervalo de páginas quando a exportação não é do documento inteiro.
- *Upstream:* `cos` é área do PdfCraft original; a correção fica registrada em `docs/` para levar ao upstream (D24).

### 2.3 `DocStore` — um armazenamento por documento (G3, G5)

```rust
// engine/src/chess/store.rs (novo) — substitui as partes copiadas de corrections.rs e reference_store.rs.
// Dois eixos separados, como já estão hoje (CorrectionStore com DirStore/MemStore, corrections.rs:716-1018):
pub trait StoreKind {                 // o QUE se guarda
    const KIND: &'static str;         // "corrections", "reference"; depois "reading", "evals"
    const FORMAT: &'static str;       // "pdfcraft-chess-corrections"
    const VERSION: u32;
    type File: Serialize + DeserializeOwned;
    fn doc(f: &Self::File) -> &DocRecord;
    fn validate_entries(f: &mut Self::File) -> usize; // descarta só entradas inválidas; devolve quantas
    fn merge(disk: Self::File, mine: Self::File) -> Self::File; // a regra de mescla do tipo
}
pub trait StoreBackend {              // ONDE se guarda
    fn list(&self, max: usize) -> io::Result<Vec<String>>;
    fn read(&self, name: &str, max: usize) -> io::Result<Option<Vec<u8>>>;
    fn write(&self, name: &str, bytes: &[u8]) -> io::Result<()>;     // atômico
    fn set_aside(&self, name: &str) -> io::Result<String>;
    type Guard<'a>: 'a where Self: 'a;                       // não `Box<dyn Drop>`: o lint `dyn_drop` reprova com -D warnings
    fn lock(&self) -> Result<Self::Guard<'_>, StoreError>;
}
pub struct DocStore<K: StoreKind, B: StoreBackend> { backend: B, read_only: bool, _k: PhantomData<K> }
impl<K: StoreKind, B: StoreBackend> DocStore<K, B> {
    pub fn load(&self, keys: &DocKeys) -> Result<Loaded<K::File>, StoreError>;   // nunca renomeia nem grava
    pub fn update(&self, keys: &DocKeys, f: impl FnOnce(&mut K::File)) -> Result<Written, StoreError>;
    pub fn rekey(&self, from: &DocKeys, to: &DocKeys) -> Result<bool, StoreError>;
}
pub struct Loaded<F> { pub file: Option<F>, pub skipped: usize, pub unreadable: Option<String> }
pub struct Written { pub rev: u64, pub skipped: usize, pub set_aside: Option<String> }
```

- **Quatro tipos, todos no motor:** `corrections` (mescla por `uid`/`rev`), `reference` (PGN do livro; decisões por endereço lido), **`games`** (a coleção de partidas, hoje um terceiro store em L7, `chess_autosave.rs`, que nem a CLI nem a `automation` alcançam; o `GameRecord` sai de `BoardState` para tipos de L0/L6 e a UI só converte; ganha identidade — `GameRecord { uid, rev, deleted }` com `#[serde(default)]` e o `uid` dado na 1ª carga de um arquivo v1 —, mescla por `uid`/`rev` com lápide como as correções, lixeira mesclada por união (`pgn` + `at`); mantém o **dono por instância** (`instance` + `held_by_another_instance`, que hoje é a única proteção, `chess_autosave.rs:478-485`), e a `automation` recusa gravar as partidas de um livro com dono vivo salvo `merge: true`; o autosave compara a `rev` do disco com a última que viu e mescla em vez de sobrescrever; o formato novo se chama `<hash>.games.json` — **nunca** `*.games`, que os builds já instalados (≤ cc2d484c) varrem e renomeiam para `.bad` quando a versão difere, de qualquer livro (`chess_autosave.rs:432-447,488-492`); o `.games` v1 é só lido pela migração, nunca regravado no formato novo) e **`cipher`** (a cifra do livro, hoje só em memória; mescla por página, V4). Uma regra de identidade só (`DocRecord::matches` delega a `DocKeys::matches`; `StoredKeys` da coleção vira `DocRecord`). Antes de qualquer mudança no formato das partidas, o store delas passa a tratar a versão maior como só leitura e deixa de pôr de lado arquivos de **outros** documentos na procura (R3-05).
- **Backends:** `MemBackend` (web e testes; herda o orçamento do `MemStore`; **um** orçamento para todas as chaves de xadrez — hoje 4 + 1 + 2 MiB em três chaves, acima da cota típica de ~5 M caracteres do `localStorage`) no motor; `DirBackend` (desktop, fora do wasm) no motor com a escrita segura de `platform::staging` descida para um módulo ≤ L6, ou em L7 e injetado. A permissão (leitura/escrita) é decidida em `automation` ao resolver a pasta (`check_roots(.., Access::Read)`); o motor só recebe o backend pronto e um `read_only` próprio — nunca o tipo `Access` (L7), nunca `engine → platform`.

- **Arquivo:** `<pasta>/<hash(DocKeys)>.<kind>.json`. Lê o nome antigo `<hash>.json` como reserva só se o `format` conferir; nunca põe de lado um arquivo de outro formato.
- **Leitura:** só lê; JSON ilegível → `unreadable` (sem renomear); entradas inválidas descartadas uma a uma e contadas.
- **A UI nunca espera a trava:** da UI, o `update` vai para o gravador do I3 (um por store, "último pedido vence"); o thread da UI só tenta a trava uma vez e, se `Busy`, enfileira — o laço de 3 s só existe na CLI e na automação (hoje o laço de 200 × 15 ms roda no Confirmar da aba Diagrama, `chess_corrections_ui.rs:295`: uma CLI segurando a pasta congelaria a janela por 3 s). Por isso o G3b (partidas) e o G3c (cifra) só entram depois do I3b. Até lá, o G3a (correções e PGN do livro) mantém na UI a espera de hoje (laço de 3 s no Confirmar, `chess_corrections_ui.rs:295`), com a nota "trocar pelo gravador do I3b". Orçamento web em **entradas**, não só em bytes (a 380 B por correção, 1 MiB guarda ≈ 2.700 — menos que os 5.334 diagramas do Polgar; o formato v2 compacto do G2 ajuda).
- **Escrita:** trava por `File::try_lock` (laço com prazo de 3 s → `Busy`, só fora da UI) num arquivo permanente de **nome novo** `<kind>.flock` — o nome antigo `<kind>.lock` é a trava por existência das versões anteriores, que a apagariam como "velha" e gravariam junto; na versão de transição o escritor toma também a trava antiga (`create_new` de `<kind>.lock`) por dentro da nova; `ErrorKind::Unsupported` (pasta de rede) cai no protocolo antigo, com aviso. Lê, mescla por `uid`/`rev`, grava pelo `write` atômico do backend. Só aqui um arquivo ilegível vai para `.bad`; temporários órfãos com mais de 1 h são apagados sob a trava — só os nomes que o próprio store cria (`^<hash>\.<kind>\.json\.tmp\d+$` e o padrão do `staging`), nunca um glob largo (o `store` pode ser uma pasta do usuário).
- **Documento protegido, uma política só:** `persist_policy(&Document) -> Persist { Never, Ask, Yes }` no motor ("cifrado → só com consentimento; sem permissão de cópia → nunca"), hoje espalhada em três lugares (autosave das partidas `chess_collection.rs:2178-2181,1233-1245`, amostras de glifo `chess_samples.rs:23-27`, instantâneos de recuperação) e que correções e PGN do livro já não seguem; todo `StoreKind`, o N5 e o pacote de revisão a consultam; o consentimento fica por documento; `AppDirs.data` é criada só do dono (como `RecoveryStore::create_dir`, `0700` no Unix).
- **Privacidade dos arquivos guardados:** nenhum caminho em claro (hoje a coleção grava `C:\Users\<conta>\…`, R7-03; o G3 troca `StoredKeys` por `DocRecord` e a migração converte os cabeçalhos). Toda resposta de escrita de ferramenta leva `set_aside`/`skipped`.
- **Índice:** `name_for` filtra `.json` antes do teto e lê só o cabeçalho `doc` (o campo vem antes do conteúdo); se medir lento com centenas de documentos, um `index.json` sob a trava.
- **Local padrão único (D17):** a pasta de dados do app, não "Recovery", resolvida em L7 (`platform`) e passada ao motor como `dir` (o motor não conhece o local), a mesma para o app e para a CLI. **Migração que mescla e pode rodar de novo** (o build antigo continua gravando em `Recovery/chess` depois da migração): para cada arquivo de origem, `DocStore::update` no destino com `K::merge(destino, origem)`, nunca cópia por cima (o mesmo livro em `Recovery` e em `.pdfcraft` daria o mesmo nome e um apagaria o outro); um `migrated.json` com (origem, tamanho, mtime, hash) gravado depois de cada mescla; a cada início, uma origem cujo (tamanho, mtime) mudou é mesclada de novo e dita ("alterado por uma versão anterior do PdfCraft; trazido"); a origem nunca é movida nem apagada; `.pdfcraft/` (uma por `--root`/CWD, que o app não conhece) migra só a da raiz em uso, por `chess_store_migrate {from}`. `--root` **restringe** (o store precisa estar dentro da raiz; a resposta diz onde procurou).
- **Ferramentas de leitura** resolvem a pasta com `Access::Read`; pasta ausente = store vazio.
- **Idempotência:** `reference.set(pgn)` com o mesmo hash mantém as decisões (bug A4-08). Com **outro** PGN, hoje as decisões são descartadas por decisão do R6e (`plano-pgn-do-livro.md:256`); a **D25** decide se passam a ser mantidas (chaveadas pelo texto do livro) com `decisions_unmatched`, ou se a UI pergunta.
- **Ferramentas só de leitura não mudam a pasta:** toda ferramenta com `read_only: true` no esquema roda sobre uma pasta que fica byte a byte igual (teste estrutural, R1-03).
- **Cópia e troca de máquina:** `chess_review_pack_export/import` (A8-19): correções + decisões + partidas, nunca o texto do PGN. Contrato: (1) a entrada passa pela mesma `validate_entries` de cada tipo e pelos seus tetos (arquivo por `read_capped`, partidas ≤ `MAX_GAMES`, PGN ≤ `MAX_PGN_LEN`, decisões ≤ `MAX_DECISIONS`, `page ≤ MAX_PAGES`); (2) o destino é o documento que o usuário escolhe, e o `DocRecord` do pacote só serve para avisar "pacote de outro livro (748 × 312 págs.)" e recusar sem `force`; (3) sem `path_hash` (FNV sem sal de um caminho com o nome da conta, adivinhável e inútil noutra máquina) e o `name` só se o usuário quiser; (4) leva `pack_id` e `reader_version`, e importar o mesmo `pack_id` duas vezes não muda nada (as partidas por `uid`); (5) importação em estágio: lê e valida tudo, depois uma `update` por tipo — uma queda entre tipos é refeita sem duplicar; (6) a interface diz que o pacote leva texto do livro nos comentários das partidas capturadas (uso pessoal); segue a `persist_policy`.
- **Web:** o mesmo `DocStore` com um backend sobre `Storage`: valor ilegível guardado em `…-v1.bad` e store em só-leitura com aviso; antes de gravar, relê e chama `K::merge` (no desktop, sob a trava, o `update` lê, muda e grava; hoje `CorrectionFile::merge` só roda em testes); o que passa do orçamento é recusado com aviso, nunca descartado.
- **UI:** as duas cópias (`chess_corrections_ui.rs` × `chess_reference_ui.rs`) saem (§2.7); fica só o backend web sobre `Storage`, que o motor recebe (I1b).

### 2.4 Onde está a página agora (G2)

- **Contrato de custo** (R8-04, R8-05): `locate` usa um mapa impressão → página construído **uma vez por estado do documento** e guardado na sessão (`ChessCache`, ao lado de `KeyState`) — o mesmo mapa que `chess_scan_corrected` usa (hoje refaz as 748 impressões a cada chamada: 83–85 ms contra 15–16 ms sem correções); `assign` agrupa diagramas e correções por página: O(D + C); nenhuma função chama `locate` por diagrama; o `KeyMemo` e o memo das impressões sobrevivem à edição para os objetos que ela não tocou (mudar o Título recalcula 0 páginas; editar uma página recalcula 1).
- **Uma regra de atribuição, no motor:** `assign(page_diagrams, located) -> (Vec<Assignment>, Vec<Unmatched>)` decide precedência, sobreposição e "uma correção por diagrama", pura; `apply` a usa, e a UI lê o `Corrected.applied` do motor para as marcas, a linha da lista, o lichess e o tabuleiro — em vez de chamar `find` por diagrama (R3-06). Um só dono das correções em memória: a sessão do motor; a UI deixa de guardar `DocCorrections.file`.
- Uma função só para localizar, `locate(entry, fingerprints) -> Located { Same(p) | Moved(p) | Changed(p) | Orphan }`, usada por `set` e `assign` — e também pelas âncoras de página das partidas (R3-04): `NodeSource`/`Source` passam a guardar a `page_fp` da página e são relocalizados pela mesma função no `chess_carry`. `follow_pages` (cache, `page_key`) segue à parte; unificar as duas chaves exigiria migrar os `page_fp` gravados (ler `fnv:` como legado) e fica fora deste plano.
- **Impressão digital vence índice** (R1-01), dentro do `assign`: no mesmo diagrama, a correção localizada por impressão digital (`Same`/`Moved`) vence a localizada só por índice (`Changed`); o desempate por `at`/`rev` vale só entre iguais. Isso vale também para as correções já gravadas, sem vizinhas.
- Cada entrada nova ganha `prev_fp`/`next_fp` (impressões das vizinhas, `#[serde(default)]`). `Changed(p)` só quando a página no índice antigo não é a vizinha seguinte gravada e ao menos uma vizinha continua no lugar; caso contrário `Orphan`: listado ("correção sem diagrama"), nunca aplicado.
- `set` substitui a viva localizada (nunca cria uma segunda). O motivo de `unmatched` diz "o diagrama já tem a correção `<uid>`" quando ele foi tomado por outra.

### 2.5 PGN fiel (onda F)

**Entrada única de bytes** (`pdfcraft_chess::pgn`):

```rust
pub enum PgnEncoding { Utf8, Utf8Bom, Windows1252, Mixed { lines_1252: usize } }
pub fn decode(bytes: &[u8]) -> (String, PgnEncoding); // BOM tirado; decide POR LINHA: UTF-8 válido fica, o resto vai por Windows-1252
```

A decisão é por linha porque coleções concatenadas (`copy *.pgn`) misturam arquivos: decidir pelo arquivo inteiro transformaria todo o UTF-8 em `Ã©` por causa de uma única partida Latin-1. Tabela própria de 32 entradas (o `chess`, L0, continua só com `cozy-chess`); os cinco bytes sem caractere (0x81, 0x8D, 0x8F, 0x90, 0x9D) viram U+0081… como no WHATWG, e controles C1 são tirados de etiquetas e comentários com um `PgnWarning`. **O hash guardado do PGN do livro** (`KeptPgn.hash`, calculado sobre o texto decodificado) muda com o F1: o "mesmo PGN" passa a ser o hash novo **ou** `text_hash(String::from_utf8_lossy(bytes))`, o legado; e o store passa a guardar também `bytes_hash` (`#[serde(default)]`). O hash legado vai no F1; o `bytes_hash`, no G4, sobre a v2 do G3a.

Os quatro leitores (Abrir PGN…, PGN do livro no app e na CLI, `chess_games`) chamam `decode`; `split_pgn`/`lex` ignoram um U+FEFF inicial (texto colado ou vindo de JSON). Exportação PGN com opção `bom` (D18).

**Um scanner, três consumidores** (`game/pgn_scan.rs`): tokens com intervalo de bytes (`Tag`, `BraceComment`, `LineComment`, `Escape`, `Open`, `Close`, `Nag`, `Word`, `Result`). `split_pgn` corta antes de um `Tag` que vem depois de um token de lances; o leitor e o `Lexer` de `doc::pgn` (enquanto existir) usam o mesmo scanner.

**Leitor em três passos puros, sobre o que já existe** (`game/pgn_read.rs`): `scan` → `parse` → `build`. O `pdfcraft_chess` já tem o modelo de tokens com variantes e o algoritmo que os joga numa partida — `FragmentItem::{Move, Nag, Comment{before}, Open, Close, Result}` (`chess/src/fragment.rs:17-45`) e `Game::graft` (`game/graft.rs:155`) —, então: `parse` produz `Vec<FragmentItem>` + etiquetas (plano, sem `Box`/`Vec` aninhados: nenhum `Drop` recursivo numa entrada `((((…`; o teto `MAX_DEPTH` vale **durante** o `parse`, e o que passa vira `PgnWarning::TooDeep` com o texto como comentário); `build` é o **núcleo do graft** sobre uma partida vazia, e o graft da captura usa o mesmo núcleo — as regras abaixo, o "já está" (A1-12, R2-02) e as de comentário ficam num lugar só. O NAG ganha tipo: `Nag::Written(code)` (o código do PGN, absoluto) × `Nag::WhiteRelative(code)` (lido do texto do livro; hoje `FragmentItem::Nag` é sempre relativo às brancas, `fragment.rs:33-35`), e a conversão é a `Nag::code_after` de F5. Regras, num lugar só:
- Uma variante nunca toma a linha principal: a resposta nascida numa variante mais funda cede o índice 0 à linha de nível menor (A1-03).
- Comentário entre `)` e o lance seguinte é o `comment_before` desse lance; variante só com comentário fica no lance de que é alternativa (A1-06, A2-02).
- Símbolos comuns não canônicos lidos e registrados (`PgnWarning::NonStandard`): `±` `+-` `∞` soltos ou colados, `e.p.`, `…`, `e8Q`, `a8=q` (A1-09).
- Etiquetas: várias por linha, recuadas; só `\\` e `\"` são escapes (A1-16).
- `Result`: `1/2`, `½-½`, `0.5-0.5` normalizados; resultado sem lances mantido (A1-21). **O `Result` lido nunca é reescrito pelo tabuleiro** (R6-04): `result()` devolve a etiqueta válida lida; o que o tabuleiro diz (mate ou afogamento no fim da linha principal) é `outcome()`, mostrado à parte; quando discordam, `PgnWarning::ResultMismatch { tag, board }`; só uma partida **criada** no app, sem etiqueta, recebe o resultado do tabuleiro. A regra dos 50 lances nunca encerra nada (A1-04).
- **`Variant` decide a leitura** (R6-05): ausente, "Standard" ou "From Position" → xadrez normal; "Chess960"/"Fischerandom" → lido no modo Chess960 do cozy-chess, que já existe (`Board::from_fen(fen, true)`, roque como rei toma a própria torre, SAN `O-O`), ou recusado com `stop_reason: UnsupportedVariant("Chess960")` se ficar fora do escopo; qualquer outra variante → recusada inteira com o nome, nunca lida como xadrez e cortada.
- **Comandos de comentário não são comentário** (R6-01): `split_commands(text) -> (prose, Vec<Command>)` no `pdfcraft_chess`, no molde do `split_drawings` (`%cal`/`%csl`) que já existe; `%clk`/`%emt` viram um selo de tempo opcional e `%eval`/`%evp` um selo de avaliação; a lista, a trilha e o comentário grande mostram só a prosa e "tem comentário" ignora comandos; o escritor mantém os comandos; as exportações de documento nunca imprimem um comando cru.
- **NAGs:** `$201` (Scid/ChessDB) também é marca de diagrama na leitura (`is_diagram`), ao lado de `$220`/`$221` (ChessPad).
- FEN tolerante: roque ajustado às peças, en passant impossível largado, relógio limitado a 100, lance 0 → 1; cada ajuste vira `PgnWarning::FenRepaired` (A1-08).
- **Nada cortado em silêncio:** cada corte ou descarte vira um `PgnWarning` com `at` — `CommentCut`, `TagCut`, `TagsDropped`, `NagDropped`, `NagReplaced`, `ResultIgnored`, `TooLong` (A1-02, A1-15). Os tetos de leitura sobem (comentário 64 KiB; o arquivo já é limitado) e passam a ser contados em **bytes** nos dois crates (A2-18). NAGs da leitura não passam por `nag_group` (o grupo é regra da paleta). O resto não jogado depois de um lance impossível vira comentário no nó onde parou, para a reescrita não perder nada (A4-05).
- `PgnReport { stopped, stop_reason: Illegal | Ambiguous | TooLong | TooDeep, warnings }`; as ferramentas devolvem `pgn_stop_reason` e os avisos por nome; a UI diz o motivo certo.

**Um escritor só** (`pdfcraft_chess::pgn::write(game, dialect)`, com `PgnDialect::{Standard, Lichess}`: no Lichess — que só conhece 23 NAGs, `Glyph.scala` — os códigos das pretas viram o das brancas e o que ele não conhece vai como o sinal num comentário `{⊥}`, sem se perder; `PlyCount` recalculado (ou tirado) quando os lances mudam; `ECO` lido mantido, mas tirado da escrita quando a linha principal muda antes do 10º lance): tokens → quebra em 79 colunas que nunca começa linha com `%` nem parte um token (token maior que 79 fica sozinho); comentário normalizado (chaves; figurinas → letras como opção); o `comment_before` da 1ª resposta sai depois das variantes do pai, logo antes do lance. Usado por `Game::pgn`, pelo escritor do documento e pelo PGN anotado (A1-11, A2-09). As partidas do documento (`doc::GameScore`, outra árvore) são escritas por `to_game` (`games/board.rs:128`), a única ponte entre as duas árvores, e os lances que ela deixa de fora (`dropped`) viram `PgnWarning`. `doc::pgn::parse` é apagado e o round trip é provado com `Game::from_pgn` (A2-18). Um teto de arquivo PGN só (hoje 32 MiB em três lugares e 16 MiB em `doc/pgn.rs:26`).

**Propriedade-guia:** para todo PGN do corpus, `read(write(read(x))) == read(x)` (normalização idempotente), e `write(read(x))` relido pelo python-chess (oráculo externo, processo à parte; o teste pula sem Python) dá a mesma árvore, as mesmas etiquetas e os mesmos comentários.

**Operações tipadas da ferramenta** (`chess_pgn_edit`): `#[serde(tag = "op")] enum PgnOp` com tipos que se validam ao desserializar (`NagCode` 1–255, `Steps` 1–`MAX_STEPS`); `apply` vira `op.apply(&mut game)`. Campos desconhecidos continuam **aceitos** (como hoje; sem `deny_unknown_fields`, que quebraria clientes); a recusa de `op` desconhecido mantém o texto `unknown op "<x>"`. O esquema é gerado por `schemars` (dependência nova, MIT, declarada em `deny.toml` e `ATTRIBUTION.toml`) ou continua à mão com um teste que compara as variantes do enum às do esquema (A7-12).

### 2.6 O livro lido sem perder texto (onda L)

- **Leitura e walk separados** (R3-01, passo A9-L0, antes de tudo na onda L). `MoveInline` guarda só o que foi lido (`number`, `text`, `san_read`, `bold`, `rect`, bandeiras do leitor). O walk é uma função pura `walk(&[PageReading], rules, overlay) -> Walk`, que devolve por inline `{status, san, fen, node, flags}` e por diagrama as suas bandeiras; `overlay` são as posições que valem mais que a leitura (correção, PGN do livro), sem reescrever a leitura. Re-percorrer é chamar de novo (propriedade: walk com overlay F2 sobre a leitura de F1 = walk da leitura de F2; walk duas vezes = uma); `add_read_page` deixa de zerar campos; `reference::link` deixa de desfazer a si mesmo (`retain` de `FromReference`, `before` do diagrama). Decisões e recasamento usam `san_read` + ordinal, nunca a SAN do walk. O "cabeçalho corrente" depende de outras páginas: é resultado do passo de documento, guardado no `Walk`, não atributo da página.
- **Endereços estáveis.** O endereço `(page, block, inline)` é gravado nas decisões do PGN do livro (`reference_store::Decision`) e usado pelo `MoveAt` (`span.rs:38,78-84`). Ele é estável para um mesmo `READER_VERSION`: o papel é atributo, e um bloco nunca é removido nem reordenado depois de o `ChessPage` existir. Quando L1, L2 ou L3 mudam a segmentação de uma página, as decisões gravadas são recasadas uma vez pelo lance (`san` + ordinal do lance na página + vizinhos); as que não casam são contadas em `decisions_unmatched` e mostradas, nunca descartadas em silêncio.
- `role: Content | Folio | GameHeader | DiagramText` como campo das variantes `Heading` e `Paragraph` do `enum Block` (`#[serde(tag = "type")]`; o campo com `#[serde(default)] = Content`), só para o que **a página decide sozinha**, em `doc/build.rs` (um papel que dependesse de outras páginas dependeria da ordem de leitura e quebraria o cache do N5); o cabeçalho corrente vem do `Walk` (L0). Walker, HTML, EPUB, Word, Markdown e o PGN anotado pulam o que não é conteúdo; o folio lido pode servir de rótulo de página no EPUB quando o PDF não tem `/PageLabels`.
- Linhas cujo centro cai dentro da caixa de um diagrama detectado são `DiagramText`, qualquer que seja a fonte (A2-06).
- Parágrafos: a dica de layout decide o tipo; a geometria decide a continuidade (A2-05). Hífen de fim de linha só se junta quando a forma colada aparece no livro ou a hifenizada nunca aparece no meio de linha (A2-15).
- Texto entre um diagrama e o lance que o resolve: guardado (`held_text`, mesmo teto dos comentários) e reprocessado quando a partida continua (colchetes contam); vira comentário inicial quando a partida começa no diagrama (A2-01).
- Toda troca de texto vira `ReviewFlag::TextRepaired { from, to, why }` (A2-10).
- **Cada fato do diagrama guardado uma vez** (R3-02, A1-14, R5-05): `DiagramReading { fen: Fen, side: SideToMove { color, source: Printed | Caption | MoveNumber | Assumed }, orientation: Option<Orientation>, … }`; o lado impresso em palavras vem do **enunciado** — `Stipulation { goal: Mate(n) | HelpMate(n) | SelfMate(n) | Win | Draw, side: Option<Color>, scope: Diagram | Page | Section }`, lido da legenda, do título do diagrama, do cabeçalho da página e do cabeçalho corrente/seção que o A9-L0 guarda no `Walk` (no Polgar o enunciado "Combinations #2" está no cabeçalho corrente), nunca do texto da análise; vocabulário como dados no `pdfcraft_chess` ao lado das palavras de `describe` ("Black to move", "Pretas jogam", "Weiß am Zug", "White to play and win", "Brancas jogam e empatam", "Mate in 2", "#2", "Mate em 2", "Matt in 2 Zügen", "h#2", "s#2", "ajudamate", "automate"…) — e alimenta o `declare_side(color, SideSource::Text | TextPageScope, …)` que já existe e ninguém produz (R6-03); a posição continua mandando: um lado impresso que contradiz a legalidade vira `SideConflict`, como já é; o lado que o walk (A9-L0) tira da legenda ("Position after: 5.dxc5", `games.rs:1181-1208`, que também tira os roques perdidos e põe a casa de en passant) ou do número do lance (`:1036-1044`) entra no `DiagramReading` derivado e é o que usam `describe`, a FEN exportada, o link do lichess e o "Quem joga" do editor — "lado não informado" só com `Assumed` (o livro imprime o lado: calar seria perder informação para quem não vê); `legal`, `accepted`, `repaired` e `side_assumed` deixam de ser `bool` ao lado de bandeiras iguais em `review` (as bandeiras da leitura passam a ser derivadas; hoje `confirm` muda as duas cópias à mão); a orientação deixa de ser lida do sufixo da URL; `describe` recebe a leitura e diz "lado não informado" quando suposto. `DiagramBlock.number` e `index` (sempre iguais, `build.rs:769`) viram um só.
- `ChessPage.text_lang` (do `/Lang` do catálogo, senão voto por palavras funcionais) separado de `lang` da notação (A2-08), os dois como `Lang`/tag BCP 47 validada, não `String` (hoje relida 10 vezes com `unwrap_or(Lang::En)`).
- Tetos com aviso: `MAX_GAMES` = 20.000 e `ReviewFlag::TooManyGames` (A2-07); `pages` ordenadas e sem repetição na entrada das ferramentas (A2-16).
- Depois de `reference::link` confirmar diagramas que mudam o FEN, chamar o walk de novo com o overlay (A9-L0) e alinhar de novo uma vez (A2-11) — sem código de re-percurso próprio.
- **Regras do walker como funções puras** (A7-10): `fn rule_<nome>(&Walker, &MoveSeen) -> Option<Placement>`; `play` = a primeira regra ligada que decide + `apply(Placement)`; `Placement` é um enum (`Comment`, `MainLine`, `Reply{to}`, `Sibling{of}`, `CloseThen{..}`, `Decide`).
- **Medida pela verdade:** `walk_bench --reference=livro.pgn` reporta confirmed/conflict/absent; decisões de regra do walker passam a ser tomadas por "confirmados", não por "válidos" (A2-03).

### 2.7 Interface: estado por documento e um só tipo de job (onda I)

```rust
// motor (L6): o estado de DOMÍNIO por documento, na sessão que já é chaveada por DocId (ChessCache, engine/src/chess.rs:231-241)
//   correções (dono único, §2.4), PGN do livro + decisões, cifra, origem do livro — o que a automation também alcança
// ui-egui (L7): só o estado de VISTA por documento
pub struct DocChess {
    results: Option<ChessResults>, text: Option<PageText>, editor: Option<DiagramEditor>, drafts: Vec<Draft>,
    list: ListState /* focus, menu, followed, review_done, picked: com DiagramAt (T2) */,
    pgn_positions: PgnPositions, pgn_source: PgnSource,
}
pub struct ChessState { docs: HashMap<DocId, DocChess>, run: Option<ChessRun>, /* + o que é da janela: abas, preferências */ }

pub struct Job<T> { slot: Arc<Mutex<Option<Result<T, String>>>>, cancel: Arc<AtomicBool>, handle: Option<JoinHandle<()>> }
// spawn(run_inline, work) · poll() -> Option<Result<T, String>> · Drop do Job = cancel.store(true)
```

**Como o `Job<T>` garante o resultado** (o padrão `Finish` que já funciona em `chess_ui.rs:392-398`): (1) o guarda RAII é a **primeira** linha do fechamento do worker, para ser solto por último no desenrolar; o `Ok` é gravado antes e o guarda só escreve se o slot está vazio; (2) o `Drop` do guarda nunca entra em pânico — `lock().unwrap_or_else(PoisonError::into_inner)`, sem `format!` de dado do `T` (pânico dentro de `Drop` durante o desenrolar aborta o processo); (3) `std::thread::panicking()` distingue "pânico" de "voltou sem resultado"; (4) falha no `spawn` grava `Err` na hora; (5) alternativa equivalente: `poll` faz `is_finished()` + `join()`, e o `Err(payload)` ainda traz a mensagem do pânico; (6) com `run_inline` (testes) o trabalho roda no thread chamador e o guarda não protege nada — o teste do pânico é unitário do `Job`, com thread real; **no wasm os jobs longos são cooperativos** (R8-03: hoje o livro inteiro roda dentro de um quadro, a aba congela e Parar não pode ser clicado): `ChessScanJob::step() -> Option<Arc<ChessPage>>` processa uma página por chamada, `chess_document_with` roda em fases (varrer por passos; texto e walk por página), e o `poll` chama `step` com um orçamento de ~50 ms por quadro, com progresso e Parar; até isso existir, "livro inteiro" na web fica recusado acima de 20 páginas, com a razão, e o parity deixa de dizer "in the background" para a web; (7) o contexto (`DocId`, `ScanStamp`) fica com quem chama: importar peças (V6) e exportar não têm `ScanStamp`. Não há `panic = "abort"` no workspace; se um dia houver, o `Job` deixa de proteger e o portão precisa saber.

- `DocChess` é criado e descartado no `sync_chess_docs` que já existe para as partidas (A5-10/A6-01). Numa aba sem detecção própria enquanto outra roda: "Detectando em outro livro… pág. 306 de 748".
- Duas vagas: a leitura de página (curta; a última pedida vence) e os jobs longos (exportar, ligar o livro), estes com Parar e progresso (A5-03, A5-12, A5-15).
- Posição efetiva num lugar: a UI lê o `Corrected.applied` do motor (§2.4), não um `effective` próprio, para a linha, a dica, as marcas, o tabuleiro e o lichess (A5-05); a URL do lichess vem do `pdfcraft_chess::lichess::analysis_url(&Position, Color)` que **já existe** (`chess/src/lichess.rs:21`, "the single place that builds it", 7 chamadores), com a orientação do `DiagramReading`, e a regra "só legal e não reparada" sobe para um lugar do motor (R3-02) — nenhuma função nova de URL.
- Com o domínio na sessão do motor, o `UiStore<K>` quase some: fica só o backend web (`Storage`) que o motor recebe.
- Pedido pendente "abrir este diagrama quando a varredura chegar" (como o `BoardAt`): "Diagrama N" da aba Texto e "Continuar editando" o usam (A5-04, A5-09).
- Autosave incremental: `GameRecord` em cache por `(uid, revision, flipped, illegal, source)`; só as partidas mudadas são recodificadas, e a gravação vai para um worker (A5-02). O `chess_carry` diz quais páginas mudaram (por `page_key`); só essas têm o `page_fingerprint` recalculado; jobs da detecção montados no worker (A5-11).
- Razões tipadas do motor (`Unreadable`, `PoorReason`, `ScanNote`) traduzidas num lugar (`chess_widgets::reason_text`) (A5-13/A6-08).
- Uma função de texto de lance, `move_text(game, node, notation)`, construída sobre `San::write(lang, style)` do `pdfcraft_chess` (as cópias da UI que trabalham sobre texto, `chess_figurines::letter`/`in_letters`, são apagadas), que lê a preferência e serve à lista, trilha, marcador, captura, linhas do motor e ao nome falado (A6-09, A6-07); o nome falado (`spoken`) desce para o `pdfcraft_chess` ao lado de `describe`, que já tem as palavras por idioma.

### 2.8 Interface: spec visual e de acessibilidade (onda I)

#### 2.8.0 Mapa das superfícies (passo I0)

Hoje cada função ganhou uma superfície própria (painel no Inspetor, aba Diagrama em coluna nova de 320 pt, aba Tabuleiro, lista de lances ao lado da página, janela própria, editor clássico, paleta flutuante). A 1366 × 768 elas se somam: Ferramentas 272 + Diagrama 320 + Inspetor/Tabuleiro 330 + trilho 48 = 970 de 1366 pt (71 %), e a página — o objeto de trabalho — cai para 29 % (R4-03). A spec parte de um orçamento de largura:

- **Orçamento de colunas por largura de janela** em **pt lógicos, depois do Tamanho da interface** (a 150 % uma tela de 1366 px tem 911 pt; a 200 %, 680 pt; trilho 48 à direita):
  - **< 1000 pt (estreito; 1366 × 768 a 150 % ou mais):** a página **ou** a coluna Xadrez, nunca as duas. Uma alternância "Página | Xadrez" (tecla F6 e um botão no cabeçalho) troca a superfície mostrada e leva o foco junto. A coluna ocupa a largura toda; nada fica fora dela (o que não cabe vai para o ⋯); rótulos quebram linha ou levam "…" com o texto inteiro no tooltip e no nome acessível (R5-04; WCAG 1.4.4, 1.4.10).
  - **1000–1599 (referência 1366 × 768 a 100 %):** **uma** coluna "Xadrez" de 360 pt. Ferramentas é recolhida automaticamente quando uma superfície de xadrez abre, com o aviso "Painel Ferramentas recolhido · Desfazer", uma vez por sessão (substitui a oferta "Mais espaço…", `chess_ui.rs:626-652`). A página fica com ≥ 940 pt.
  - **1600–1919:** duas colunas de 360 pt: Xadrez + uma segunda (Tabuleiro **ou** Diagrama).
  - **≥ 1920:** duas colunas de 420 pt; Ferramentas pode ficar aberta; o tabuleiro do editor vai até 380 pt (R4-13).
- **A coluna Xadrez** tem três abas com papel Tab: **Diagramas · Texto · Tabuleiro**. Diagramas é mestre-detalhe: abaixo de 1600 o editor **substitui** a lista na mesma coluna ("‹ Diagramas" e Esc voltam); acima de 1600 abre na segunda coluna. O modo Ler e a janela destacada usam **a mesma** coluna e o mesmo mestre-detalhe: nenhuma superfície é exclusiva de um modo (R4-01, R4-12).
- **Zona de avisos:** uma por coluna, entre o cabeçalho e o conteúdo, um aviso por vez (oferta do PGN, modelo ausente, "o documento mudou"); os outros entram em fila. A mesma contagem aparece uma vez (hoje "999" três vezes: título, resumo e chip).
- **O tabuleiro não rola:** regiões fixas — cabeçalho 28 · tabuleiro · navegação 28 · motor (0 ou 3 × 20; uma linha por variante, truncada com "…", completa no tooltip) · trilha 20 · lances (preenchem o resto) · barra inferior 24 com **Partida ▾ · Comentário · No livro (n) · FEN/PGN ▾** (o "No livro" é o N3; abre a lista no lugar dos lances, Esc volta), que substitui as seções abaixo da lista. Os ajustes de captura vão para o menu da ferramenta Capturar.

Esboços a 1366 × 768 (1360 × 705; 619 pt úteis de altura; página ≈ 950 pt à esquerda). As alturas vêm das capturas, não de um protótipo: o I0 começa por um kittest de leiaute que mede cada região.

```
(a) Xadrez › Diagramas (lista)                 (b) Xadrez › Diagrama (detalhe, mesma coluna)
┌ Xadrez ───────────────────────── ⋯  × ┐ 32   ┌ ‹ Diagramas  Pág. 13 · 1 de 2   ‹ 3/12 › ┐ 32
│ [Diagramas]  Texto   Tabuleiro         │ 30   │ [Lido | Original]     ● Não confirmado    │ 28 (linha reservada)
│ [⛶ Detectar no livro ▾]  [⇩ Exportar…] │ 36   │      ┌───────────────────────┐            │
│ 999 diagramas · 12 a revisar [Revisar] │ 24   │      │ tabuleiro 296 × 296   │  "Original" mostra a
│ (Todos 999)(A revisar 12)(Corrig. 3) ▾ │ 28   │      │ (ou o original, no    │  página nesta mesma
│ □ Acompanhar a página                  │ 24   │      │  mesmo quadrado)      │  caixa (UV do canvas)
│ ┄ zona de avisos (1 por vez) ┄┄┄┄┄┄┄┄┄ │      │      └───────────────────────┘            │ 296
│ Pág. 13 ─────────────────────────────  │ 24   │ ♔♕♖♗♘♙ ♚♛♜♝♞♟ ⌫   (uma linha, 26 pt)      │ 36
│ ✓ Diagrama 1              Corrigido ⋯  │ 32   │ Quem joga [Brancas|Pretas]  Número [12]   │ 28
│ ✓ Diagrama 2                   Lido    │ 32   │ ▸ Detalhes (roque, FEN, virar, restaurar) │ 24
│ … ≈ 13 linhas visíveis                 │      ├───────────────────────────────────────────┤
└────────────────────────────────────────┘      │ ✓ Posição legal    ⋯   [✓ Confirmar e seguir ▾]│ 44 fixo
                                                └───────────────────────────────────────────┘ ≈ 520 de 619

(c) Xadrez › Texto                              (d) Xadrez › Tabuleiro (estudo; trilho de 32 pt dentro)
┌ Pág. 13 · camada de texto   [Ler com OCR] ⋯┐ 32  ┌──┬ 6…Rf8 · Pretas jogam  [◔ Analisar]  ⋯ ┐ 28
│ PGN do livro: 8 confirmados · 0 em conflito│ 20  │↖ │▌┌────────────────────┐                │
│ ───────────────────────────────────────── │     │▦ │▌│ tabuleiro 280      │ barra de       │ 280
│ texto da página, um galley por parágrafo  │     │✂ │▌└────────────────────┘ avaliação 8 pt │
│ (≈ 540 pt, sem os 6 botões de hoje)        │     │! │ «  ‹  ›  »  ⇅                       │ 28
│                                            │     │= │ +2,14 7.Dxd5 De7+ 8.Ce2 …   (só com │ 60
│ ⋯ = Origem do livro, Carregar/Alinhar/     │     │… │ +1,33 7.Ce2 Cf6 8.0-0 …      motor) │
│     Remover o PGN do livro, Exportar…      │     │  │ Linha principal › 5…Bxc5 › 6…Rf8   │ 20
└────────────────────────────────────────────┘     │  │ lances (preenchem: ≥ 7 linhas com  │ ≥ 160
                                                   │  │ motor, ≥ 10 sem)                    │
                                                   │  │ Partida▾ Comentário No livro(3) PGN▾│ 24
                                                   └──┴─────────────────────────────────────┘

(e) Modo Ler: a mesma coluna Xadrez (a, b, c, d) no painel direito do modo Ler; nenhuma superfície some.

(f) Janela destacada 960 × 704
┌ Xadrez — Beat the French…                                   [Voltar para a janela principal] ┐
│trilho│ cabeçalho do tabuleiro                         │ [Lances] Diagramas  Texto  (340 pt)  │
│      │ ┌──────────────────────────────┐               │ lista de lances (preenche)           │
│      │ │ tabuleiro ≈ 540              │               │  — ou a lista/detalhe de diagramas   │
│      │ └──────────────────────────────┘               │  — ou o texto                        │
│      │ « ‹ › »   motor (3 linhas)                     │                                      │
└──────────────────────────────────────────────────────────────────────────────────────────────┘
```

#### 2.8.1 Regras

- **Medir antes de desenhar.** Toda largura de conteúdo sai do retângulo do contêiner (`body_rect`), nunca de `available_width()` depois de outro conteúdo; rótulos de botão com nome de arquivo usam `widgets::fit_line` e o nome inteiro vai no tooltip e no nome acessível; avisos que aparecem e somem têm a linha reservada (sem salto de leiaute); `ScrollArea` do painel de xadrez reserva a largura da barra.
- **Aba Diagrama:** dica da 1ª vez em `horizontal_top` com o × no canto; o tabuleiro já sai do `body_rect` (#254, `board_room`/`board_side`); falta reservar a linha do aviso. **Original / Lido** é uma alternância **no mesmo quadrado** do tabuleiro, com a mesma posição e o mesmo tamanho: o Original é a sub-região da página que o canvas já renderizou (desenhada por UV) ou renderizada sob demanda — não um cache de recortes (o recorte da leitura existe só no worker, em cinza, e é descartado; 999 recortes em cinza seriam ≈ 470 MB a 220 dpi, ≈ 3,5 GB a 600 dpi). O Original é desenhado por `RenderRequest.tile` (o renderizador já tem recorte, `render/src/raster.rs:157`) no worker de render, um pedido em voo, cache por (doc, página, diagrama, escala) — nunca a página inteira no zoom do diagrama. Com a coluna ≥ 2 × tabuleiro + 24 os dois ficam lado a lado. Tecla **Alt+O** (o `O` sozinho já é a "Área de OCR" do trilho, `chess_ocr_area.rs:114`), conferida pelo teste de colisão do mapa de teclas único (I18) (A6-14).
- **Aba Texto — dois eixos, só com o que o egui faz.** Cada parágrafo é um `LayoutJob` (um galley) com seções por lance; um foco por lance e por diagrama; prosa não focável; miniatura do diagrama com nome "Diagrama 2, abrir" (A6-06, A6-07, A6-23).

  | Eixo | Valores | Codificação |
  |---|---|---|
  | Estado do walk (os 7 `MoveStatus`) | válido · ilegal · ambíguo · fora de sequência · não alcançado · não conferido · legenda | cor de **texto** com token (`text` · `danger_text` · `danger_text` · `warning_text` · `warning_text` · `text_muted` · `accent_text`) **e** um marcador de forma pintado antes do lance (nenhum · "×" · "×?" · "?" · "?" · "·" · "▸"); o teste de contraste enumera `MoveStatus` por `match` exaustivo (variante nova sem token não compila); nenhuma cor fixa (hoje `Caption` em `#3B82C4` dá 4,06:1 e 3,72:1, R5-12) |
  | Ligação ao PGN do livro | confirmado · lido do PGN · em conflito (R6g do `plano-pgn-do-livro.md`) | o sublinhado **contínuo** de hoje (`TextFormat::underline`) para "lido do PGN"/"PGN escolhido"; itálico para conflito sem decisão |

  Nada de sublinhado ondulado ou pontilhado: o `TextFormat` do epaint 0.36 só tem traço contínuo. As duas informações também vão no nome falado: "6. Bispo b5, xeque, não conferido, lido do PGN".
- **Cores.** Proibida cor fixa em **texto, fundo e borda de controle** de `chess_*.rs`; um teste textual procura `Color32::(GRAY|from_rgb|from_gray|from_rgba_unmultiplied)` fora de uma **lista fixa de cores de domínio**, cada uma com contraste testado contra a casa clara e a escura: marcas e setas na convenção do lichess (`chess_board_ui.rs:147-151`), fichas de NAG (`:1406-1414`, já a 4,5:1), ficha de avaliação (`chess_study_ui.rs:662-663`, com borda `border_strong` no escuro) e o ponto de quem joga; a tinta `WHITE` das imagens não é cor. Texto ≥ 4,5:1 e não-texto ≥ 3:1 em claro e escuro, medidos por teste; token `border_strong`; barra de progresso de 6 pt com `accent` sobre `border` e estimativa depois das 8 primeiras páginas; "Brancas jogam" em `text` com o ponto de quem joga (vermelho só para "Posição ilegal" e mate; hoje é verde/vermelho como estado de diagrama, R4-05).
- **Avisos, anúncios e desfazer** (R4-10, R4-09, R5-02). Um `announce(level, text)` sobre o padrão de `chess_diagram_tab.rs:222-229`: `Status`/`Polite` para informação, `Alert`/`Assertive` para erro, e um nó novo por mensagem (o AccessKit só emite `LiveRegionChanged` quando o nome muda ou o nó nasce: dois avisos iguais seguidos não seriam lidos). As 99 chamadas `notify*` de `chess_*.rs` são classificadas uma a uma: informação → `notice` com `Status`; erro → `callout` de perigo com `Alert`, que fica até ser dispensado (nunca some em 3,5 s; WCAG 2.2.1, 4.1.3). **O lance mostrado é anunciado a cada mudança**, inclusive por teclado e clique, com o nome falado do I10: "6. Bispo b5, xeque · variante 2 · com comentário · sinal: bom lance". Um componente `notice(text, action: Option<(label, Action)>)`, ancorado na base da **área da página** (nunca cobre rodapés de painel), 5 s sem ação e até a próxima ação do usuário com ação. **Toda mudança de estado do usuário** dá um aviso com **Desfazer**: confirmar, esquecer, redesenhar no PDF, pôr link, aceitar o PGN do livro, alinhar o livro. O fim da detecção diz "999 diagramas · 12 a revisar" com **Revisar agora**. Erros e alertas usam `callout(kind, text)` com ícone, cor de texto e fundo `*_soft` dos tokens — nunca `text_muted` + `.small()`.
- **Acessibilidade.** Modais: papel `Dialog` com `modal` (AccessKit `set_modal`), nome = título, foco no primeiro controle **não destrutivo**, Tab preso no diálogo, Esc fecha, e ao fechar o foco volta ao controle que o abriu (`widgets::show_modal`); no mestre-detalhe do I0a, abrir o detalhe leva o foco ao tabuleiro do diagrama, e "‹ Diagramas"/Esc o devolvem à linha de origem (o `focus_list` de `chess_diagram_keys.rs:44` já faz a volta); recolher Ferramentas nunca deixa o foco num controle escondido; todo `RadioButton` tem um pai `RadioGroup` nomeado ("Quem joga"); itens de menu com papel `MenuItem` e o popup com papel `Menu`; abas com papel Tab; todo controle com nome (`labelled_by`); nome acessível contém o rótulo visível (WCAG 2.5.3); itens de menu fecham o menu também por AccessKit — **estendendo o `widgets::menu_item(ui, label, shortcut)` que já existe** (`widgets.rs:299-301`), não um homônimo; teste genérico "nenhum nó focável sem nome no painel de xadrez". **Alvo mínimo de 24 × 24 pt** (WCAG 2.2 2.5.8; hoje Parar 22 pt, Dispensar 20 pt); botões-ícone em **dois** tamanhos (24 inline nas linhas, 28 em cabeçalhos e no trilho; hoje seis). Garantia estrutural como já se faz com `checkbox`/`radio`: `clippy.toml` `disallowed-methods` para `egui::Modal::new` (força `show_modal`) e `egui::ProgressBar::new` (força um `widgets::progress` com tokens).
- **Um ícone, um conceito** (R4-07), numa tabela conferida por teste sobre `chess_*.rs`:

  | Ícone | Hoje | Fica com |
  |---|---|---|
  | `layout-grid` | Tabuleiro (cabeçalho) e Montar posição (trilho) | Montar posição; o Tabuleiro ganha `grid-3x3` com rótulo |
  | `list-ordered` | Lista ao lado da página e Capturar como linha | Lista ao lado; Capturar como linha ganha outro |
  | `circle-dashed` | estado "Não lido" e vazio da aba Diagrama | Não lido; o vazio usa `square-dashed-mouse-pointer` |
  | (Parar) | "×", ícone de fechar | `circle-stop` (acrescentado ao conjunto Lucide, ISC, já atribuído); `square` não, porque é o ícone de reserva de nome inválido (`icons.rs:16`) |

- **Escala tipográfica do xadrez:** 15 semibold (título da superfície) · 13 semibold (seção) · 13 (corpo) · 12 (secundário) · 11 (só metadados); hoje são oito tamanhos. **Espaçamento:** múltiplos de 4; margem de coluna 12; linha de lista 32; linha compacta 28.
- **Tabuleiro em Navegar:** "Quem joga" sai (já fica desabilitado com lances); o campo do 1º lance **fica**, porque é a correção do número de uma partida mal lida (`chess_board_ui.rs:2131-2160`), e vai para **Partida ▾ › "A partida começa no lance: [1]"** com o tooltip de hoje. Na aba Diagrama o rótulo vira "Número do lance:" (hoje "Lance nº:" nomeia duas coisas).
- **Linha da lista:** o estado como ícone + palavra curta ("Corrigido", "PGN"), o completo no tooltip e no nome acessível; com rascunho, só "Não confirmado" + ponto. "Esquecer a correção" no ⋯ do rodapé fica depois de um separador, em `danger_text`, com confirmação quando há rascunho.
- **Descoberta** (R4-02): comandos `chess.board.show` "Tabuleiro de xadrez", `chess.analyze` "Analisar a posição", `chess.export` "Exportar livro de xadrez…", `chess.shortcuts` "Atalhos de xadrez" (tecla **F1** no escopo de xadrez — o `?` já é o sinal "lance ruim" digitado, `chess_tools.rs:727-729` —, uma folha gerada do mapa de teclas único do I18); menu **Xadrez** na barra de menus; Ferramentas › Xadrez com Detectar diagramas, Painel de xadrez, Tabuleiro, Capturar trecho, PGN do livro e Exportar livro; o ícone de xadrez do trilho aparece quando o documento tem fonte de xadrez, PGN irmão ou detecção (hoje só com detecção), uma vez com um ponto.
- **Exportação com diálogo** (R4-04): formato, intervalo de páginas, origem das partidas do PGN, diagramas como partidas e **idioma do livro** (torna o A2-08 visível e corrigível), com o resumo do que sai ("120 partidas · 999 diagramas (12 não conferidos) · 748 págs. · inglês") e o aviso de pendências dentro do diálogo; até o N2, uma frase: "As partidas que você editou no tabuleiro estão em Salvar todas como PGN…".
- **Linhas do motor:** fonte proporcional e a notação da preferência (I10), não `monospace` (`chess_study_ui.rs:664,670`).

#### 2.8.2 Textos pt-BR (tabela exata)

| Onde | Hoje | Novo |
|---|---|---|
| Ferramentas, catálogo, comando | Procurar diagramas de xadrez · Painel de diagramas de xadrez | Detectar diagramas · Painel de xadrez |
| Título do painel | Diagramas de xadrez | Xadrez |
| Chips (estados) | Todas 999 · Lido 54 · … | Todos (999) · Lidos · A revisar · Não lidos · Com erro · Conferidos · Corrigidos · Confirmados pelo PGN |
| Botão principal sem fila | Só confirmar (e ▾ com "Confirmar e seguir" desabilitado) | **Confirmar**, sem ▾ |
| Depois de confirmar, sem mudança | Só confirmar | **Confirmado** (desabilitado até a posição mudar) |
| Aba Texto | Ligar o livro inteiro · … de novo | **Alinhar o livro ao PGN** · Alinhar de novo |
| Janela própria | Encaixar · Trazer de volta | **Voltar para a janela principal** (as duas) |
| Prévia da captura | Encaixar em · Idioma | **Inserir em** · **Letras das peças** |
| Exportações | Exportar… (cabeçalho) · Exportar… (lista de partidas) | **Exportar livro…** · **Salvar todas como PGN…** |
| Fim da detecção | Diagramas encontrados: 999 | 999 diagramas · 12 a revisar [Revisar agora] |
| Detectar, ação principal | Detectar diagramas (dispara 748 págs.) | Detectar no livro (748 págs.) · "≈ 7 min" depois das 8 primeiras páginas |
| Preferências | Último lance: Pintadas · "O comentário do lance mostrado, grande sob o tabuleiro" · "Um lance em que o ponteiro para mostra a sua posição" | Destacar o último lance: Casas pintadas / Anel / Seta · Mostrar o comentário do lance, em letra grande, sob o tabuleiro · Mostrar a posição ao passar o ponteiro sobre um lance |
| Diversos | "12691 lances"; selo "Pronto"; id interno no tooltip; conjuntos de peças em inglês | "12.691 lances" (locale); sem selo nem id; nomes comuns traduzidos (os próprios ficam: Chessnut, RhosGFX) |

O teste de glossário (I14) recusa "Ligar o livro", "Encaixar" no sentido de *Dock*, "Só confirmar" como único botão, "Procurar diagramas" e "glifo" para NAG.

#### 2.8.3 Matriz WCAG 2.2 AA (app) e EPUB Accessibility 1.1 (exportações)

Alvo declarado: **WCAG 2.2 AA** no painel de xadrez e **EPUB Accessibility 1.1** nos livros exportados. "Hoje": passa · falha · n/t (não testado). "TA real" = teste com leitor de tela e lupa reais (D26).

| Critério | Hoje | Passo | Teste automático | TA real |
|---|---|---|---|---|
| 1.1.1 Conteúdo não textual | falha (tabuleiro de análise = FEN; miniatura sem nome; sinais só com `title` nas exportações) | N7a, I9, X3 | kittest: nenhum nó `Image` sem nome; exportação com `aria-label` nos sinais | sim |
| 1.3.1 Informação e relações | falha (abas com papel de botão; `RadioButton` sem grupo; menus sem papel) | I11, I15 | kittest de papéis | — |
| 1.4.1 Uso de cor | falha (aba Texto, estado só pela cor; marca de revisão no HTML só cor) | I9, X3 | marcador de forma presente por `MoveStatus` | — |
| 1.4.3 Contraste (texto) | falha (cinza 2,61:1; legenda 4,06:1) | I9 | contraste por token e por `MoveStatus` | — |
| 1.4.4 Redimensionar texto · 1.4.10 Reflow | falha a 200 % (R5-04) | I0 | kittest de leiaute também a 680 × 352 pt | lupa |
| 1.4.11 Contraste não-texto | falha (caixa 1,39:1; barra 1,19:1) | I8, I11 | contraste de bordas e indicadores ≥ 3:1 | — |
| 1.4.13 Conteúdo em foco/ponteiro | falha (prévia de lance sem Esc) | I10, N1 | Esc fecha a prévia sem mover o foco | — |
| 2.1.1 Teclado | falha (sem lance pelo teclado no tabuleiro; escopo que só sai por clique) | N7a, I18 | kittest só com teclado em cada jornada (Q5) | sim |
| 2.1.2 Sem armadilha de teclado | n/t (escopo) | I18 | Tab/tecla F6 saem de toda superfície | — |
| 2.1.4 Atalhos de um caractere | falha (sem desligar) | I18 | preferência desligada → letras não agem | — |
| 2.2.1 Tempo ajustável | falha (avisos de erro somem em 3,5 s) | I17 | `callout` de erro fica até dispensar | — |
| 2.4.3 Ordem do foco · 2.4.7/2.4.11 Foco visível e não encoberto | n/t (anel não visto nas capturas com a janela sem foco; paleta flutuante pode cobrir) | I11 | Tab por todos os focáveis com a janela em uso: anel ≥ 2 pt e ≥ 3:1; retângulos não cobertos | lupa |
| 2.5.3 Rótulo no nome | falha ("Agora não") | I15 | nome contém o rótulo visível | — |
| 2.5.7 Movimentos de arrasto | em parte (peças por clique-clique e divisor por setas passam; setas e círculos só por arrasto) | N7a | desenho por campo e por Alt+clique | — |
| 2.5.8 Tamanho do alvo | falha (20 e 22 pt) | I15 | todo nó clicável ≥ 24 × 24 | — |
| 3.1.1/3.1.2 Idioma (exportações) | falha (`lang` da notação, A2-08) | L6 | `lang` do texto | — |
| 3.3.1 Identificação de erro | em parte (erros em texto cinza pequeno) | I17 | `callout` com `Alert` | sim |
| 4.1.2 Nome, papel, valor | falha (≥ 15 nós sem nome; grade com FEN no nome; células sem `bounds`) | I11, I15 | nenhum nó focável sem nome; `bounds` em toda `GridCell` | sim |
| 4.1.3 Mensagens de status | falha (um só nó vivo no app) | I17, I15 | `role()`/`live()` depois de cada ação | sim |
| EPUB A11y 1.1 | em parte (metadados constantes e às vezes falsos) | X3 | Ace by DAISY e epubcheck como oráculos opcionais (Q2) | — |

O `ui inspect` do canal de controle passa a serializar `live`, `modal`, `expanded`, `has_popup`, `active_descendant` e `bounds` dos filhos (Q5), para que um agente confira a matriz no app instalado.

### 2.9 Novas funcionalidades: contrato do MVP (onda N)

- **N1 — Lance clicável no PDF.** `chessvision::doc::move_at(&ChessDoc, point) -> Option<(MoveAt, fen_after)>` (função pura), reaproveitando o `MoveAt { page, block, inline }` que já existe (`engine/src/chess/span.rs:38`, argumento de `read_book_game`) — o tipo desce para o `chessvision` se preciso; nunca um segundo `MoveAt`. No ramo "sem diagrama" do duplo clique, a página é lida como na aba Texto (sob demanda) e `read_book_game` põe a partida no tabuleiro naquele lance. Prosa continua muda. Ferramenta `chess_move_at {doc, page, x, y}`. Fase 2: prévia do tabuleiro após 400 ms de ponteiro parado (desligável).
- **N2 — Uma verdade das partidas para todas as exportações.** `reference::link` aceita várias referências com prioridade (coleção do usuário > PGN do livro > leitura); HTML, Office e EPUB usam o lance ligado quando o lido não jogou, marcado, sem trocar o texto impresso. `reference_pgn` e `games_pgn` em todas as `chess_export_*`. Depende de F4 (escritor único) e G2.
- **N3 — Esta posição no livro.** Sobre o `BookKey` de T1 (colocação + vez; com lado suposto, só a colocação — os 12 diagramas do Polgar e os 37 do Karpov lidos têm o lado suposto); índice posição → ocorrências (página, diagrama/lance, partida) a partir do PGN do livro (instantâneo) ou da leitura do livro; seção "No livro" no tabuleiro com as continuações **jogadas de novo na posição real** do tabuleiro (uma continuação que ali é ilegal — o roque já se perdeu, o en passant não existe — aparece desabilitada com o motivo), cada uma com o seu papel no livro (linha principal em negrito × variante), o sinal que o autor deu e a página; a contagem é informação secundária (num livro de repertório a linha recomendada aparece uma vez e um erro típico pode aparecer dez); `chess_position_find {doc, fen|moves}`.

### 2.10 Compatibilidade entre versões dos arquivos gravados

O usuário guarda builds antigos ao lado do atual (`dist/app/anterior-…`), e as sessões de agente rodam CLIs de outros worktrees na mesma pasta de dados. O serde descarta campos desconhecidos, e toda escrita de store regrava o arquivo inteiro. Por isso:
- **`READER_VERSION` passa a existir** (hoje 0 ocorrências no código): `doc::READER_VERSION: u32` sobe em todo PR que muda a segmentação da leitura (A9-L0 a L3); é gravado nos arquivos que guardam endereços (`ReferenceFile.reader_version`, `#[serde(default)] = 0`) e na chave do N5; o recasamento das decisões (§2.6) roda na carga quando a versão gravada é menor, e grava a nova; quando é **maior** que a do código (um build anterior entre dois PRs que sobem a versão — L1b, L2, L3 —, todos lendo `reference` v2), o arquivo abre só para leitura, como `Read::Newer`, e nenhuma decisão é aplicada por endereço (todas contadas em `decisions_unmatched`, motivo "gravadas por uma leitura mais nova"). A regra nasce no L1b, o primeiro PR que grava `reader_version`.
- **Campo de que o dado depende sobe a versão do formato**: corrections 1 → 2 quando G2 acrescenta `prev_fp`/`next_fp`; **reference 1 → 2** no primeiro PR entre G4 (`pgn_san`), F1 (`bytes_hash`) e L1 (recasamento) — senão o build antigo descarta os campos novos ao regravar e aplica endereços recasados à **sua** segmentação (`Decision::is_for` compara só `(page, block, inline)` + `san`, e com uma SAN comum, "Nf3", a decisão cai noutro lance); partidas no nome novo `<hash>.games.json` (§2.3). O build antigo já abre um arquivo de versão maior como só leitura (`Read::Newer` → `read_only`, recusado em `changed`: `corrections.rs:328-331,758-761,774-776`), em vez de regravá-lo sem os campos novos.
- **Trava com nome novo** (`<kind>.flock`); a trava antiga é tomada por dentro **enquanto um build antigo puder gravar aquele tipo** — quando o tipo sobe de versão (só leitura no antigo), ela sai. Nunca "por uma versão".
- **Preferências:** `persist()` parte do JSON carregado e sobrescreve só as chaves que conhece (CLAUDE.md "Preserve unknown data"; hoje um build mais velho apaga as preferências que um mais novo gravou, R7-05). Isso só protege dos builds futuros: os já instalados continuarão voltando ao padrão as preferências novas do plano (atalhos de uma tecla, notação, escolhas do diálogo de exportação) — fica dito nas notas da versão.
- **Cifra do livro (V4) no `cipher_out` da CLI:** o JSON continua gravando o agregado antigo (`entries`) ao lado de `pages` por uma versão, porque `BookCipher` é `#[serde(default)]` e um build antigo leria um layout sem `entries` como tabela vazia, sem erro. Regra do legado: o agregado antigo só conta para um símbolo enquanto nenhuma página nova tiver evidência dele (reler uma página do agregado não pode ser subtraído — não há como "não duplicar" o passado).
- **Endereços `(page, block, inline)`** estáveis por `READER_VERSION`, com recasamento das decisões gravadas (§2.6).
- **Hash do PGN do livro** aceita o legado (§2.5, F1).
- **Coleção de partidas** (`.games`): hoje uma versão diferente é posta de lado em qualquer documento (R3-05); passa a ter a mesma política das correções (versão maior → só leitura, nunca tocar arquivos de outros documentos) (passo G12) **antes** de qualquer mudança no seu formato; o `StoredComments` continua gravado enquanto o arquivo for `*.games` v1 e só sai junto com o nome novo (G3b; R9-01).
- **Cifra:** com a cifra virando `StoreKind` (§2.3), o arquivo guardado tem versão própria e o build antigo nunca o lê; o agregado antigo ao lado só continua no JSON de `cipher_out` da CLI, por uma versão.
- **Teste por formato:** cada passo que muda um formato gravado (G2a, G3a, G3b, G3c, G4, V4b, L1b, I3a; e o T2a se a `Decision` mudar de forma) congela as structs serde da versão N em `tests/compat/<kind>_vN.rs` (cópia literal do commit que a introduziu) e testa novo → `vN::from_slice` → `vN::to_vec` (o que o build antigo faz ao regravar) → novo, sem perda; com `PDFCRAFT_OLD_CLI` (ex.: `dist/app/anterior-…/pdfcraft-cli.exe`), o mesmo pela CLI antiga — sem a variável o teste pula **com aviso** (e falha sob `PDFCRAFT_REQUIRE_MODELS`).

## 3. Roadmap

Branches `feat/chess-a9-<id>` (ou `fix/`). Alvo < ~800 linhas alteradas por PR. Em todo passo: teste visto falhando antes, entrada em `parity/` e ferramenta headless quando houver função nova, i18n completo, capturas a 1366 × 768 claro e escuro para o que for visível, `cargo xtask chess-gate` (Q1a) verde antes do PR.

### 3.0 Passo 0 e o núcleo de 10 passos

**Passo 0 (dois PRs; S).** **P1 — feito em 2026-11-11** (o PR deste plano): o plano; `docs/chess/analise-9/` com A1–A8, R1–R10, `log.md`, `brief.md`, `review-brief.md`, `coerencia.py` (confere ficha × grafo × ondas × núcleo; rode a cada versão do plano), `dry.txt` e `parity_open.txt`; o texto da ajuda de 2026-10-31 extraído por `pdfcraft-cli text` em `docs/chess/ajuda/texto-2026-10-31.txt`, como fonte pt-BR do E1 (a fonte HTML e as imagens se perderam numa pasta temporária; os geradores `gen_book.py`, `toc.py` e `sheet.py` estão em `C:/Python-Chess2/analise-9-local/ajuda/`), e `docs/chess/ajuda/PENDENTE.md`. As provas foram copiadas da pasta temporária da sessão para `C:/Python-Chess2/analise-9-local/`, e a v0 sem commit saiu do worktree de leitura. **P2 — antes do Q1a** (faixa A): em cada uma das 20 entradas de xadrez do `parity/`, uma frase "Missing (plano A9): …" com os achados C e I abertos contra ela (R10-04; o `saida-parity.txt` da R10, em `analise-9-local/R10/`, já os cruza); `cargo xtask parity` e os testes do `xtask` verdes. Cada passo tira a sua frase quando fecha o achado.

**O núcleo** — o que vale fazer mesmo se o resto parar (≈ 16 PRs, ≈ 17 dias de agente, ≈ 2 semanas com as três faixas das ondas 0–1):

| # | Passo | Fecha | PRs · dias |
|---|---|---|---|
| 1 | Q1a: portão local, etapas 0–3 | A7-03, A7-04, A7-05, A7-06 (protege os outros nove) | 1 · 1,5 |
| 2 | V1 + V8: entradas hostis que abortam | A3-01, R2-01, R7-01 (os 3 abortos) | 2 · 2 |
| 3 | G1: `/ID` | A4-03 | 1 · 1 |
| 4 | G7a: confinamento, sobrescrita e `chess_games` sem cortar | A4-04, A4-11, A4-05, A4-10, R7-04, R7-06 | 1 · 1 |
| 5 | F1: BOM e Latin-1 | A1-01, A1-10 | 1 · 1 |
| 6 | G2a: a correção nunca cai noutro diagrama | A4-01, R1-01, A4-02 | 1 · 1,5 |
| 7 | G12 + G8: coleção e lixeira sem perda | A5-01, A5-07, R1-02, R3-05 | 2 · 2 |
| 8 | F2 → F3: leitor sem cortes e sem trocar a linha principal | A1-02, A1-03, A1-05, A1-12, R2-02 | 3 · 4,5 |
| 9 | G11: o livro lido uma vez por sessão | R8-01 (304 s → ≈ 5 s por exportação) | 2 · 1 |
| 10 | I15: rápidos e visíveis | R4-01 (corrigir no modo Ler: impossível → igual), A6-02/A6-03 (recusar a oferta: impossível → 1) | 2 · 1,5 |

Fecha os **3 de 3** abortos, **18 dos 29** itens da métrica de perdas e duas das três jornadas "impossíveis" do §5 (a terceira, jogar um lance só pelo teclado, é o N7a, onda 5), e tira ≈ 5 minutos de cada exportação. **Logo depois (11–14):** G3a (+ A4-06, A4-07, A4-13), I1a (A6-01, o último C de resultado errado), A9-L0 → L2 (A2-01, C) e V6a (R7-02); com eles, 24 de 29. **Pode esperar sem risco de perda:** L9, V9, I6, F6, Q3b, o índice das Preferências do I14, X1b e toda a onda N.

### 3.1 Grafo (acíclico; "A ─► B" = B depende de A; a ficha §3.4 é a fonte, e este bloco é conferido por `analise-9/coerencia.py`)

```
P ─► Q1a ─► (todo o resto) ;  Q1a ─► Q2 ─► Q1b ─► {Q3a, V2, V10a} ;  Q2 ─► L4 ─► L9
Faixa A (motor e automação)
  G1 ;  G7a ;  G2a ─► G2b ─► G3a ─► {G3b, G4, G5a, G10, L1b, N5a} ;  T3 ─► {G3a, G6, G5a, I13} ;  G11 (motor + UI)
  I2 ─► I3b ─► G3b ─► {G2c, G5b} ;  G12 ─► G3b ;  G5a ─► G5b ;  G3a + I3b + V4a ─► G3c ─► V4b
  F3 ─► F4 ─► {F7, I3a, N2, N9, N10} ;  F2b ─► {F5, F6, Q3b}
Faixa B (UI)
  G12 ─► G8 ;  G12 + F4 ─► I3a ;  I15 (com I7a) ─► I14 ;  I18 ─► {I16, Q5}
  G2b ─► I1a ─► {I11a, I1b} ;  I1b ─► {I2, T1b, T2b} ;  I1b ─► I0a ─► I0b ─► I0c ─► {I8, I9, I13, I14, I16, I17, N1, N3, N7b} ;  I0a + I11a ─► I11b
  I2 ─► {I4, I7b, V6b, V10b} ;  V10a ─► V10b ;  I9 ─► I10 ─► I17 ─► {N7a, X2} ;  I10 ─► G9 ;  G3a ─► G10 (cria o `notice`)
Faixa C (crates `chess` e `chessvision`)
  V1 ─► V8 ;  F1 ─► F2a ─► F2b ─► F3 ;  Q4a
  T1a ─► {T1b, T2a, G2b, A9-L0a} ;  T2a ─► {T2b, G2c} ;  A9-L0a ─► Q3b ;  A9-L0a ─► A9-L0b ─► {L1a ─► L1b, L2, L3, L4, L5, L6a ─► L6b ─► L6c, L7 (+ G7b), L8}
  L6b ─► {X3, G9} ;  L6c ─► N4 ;  L5 ─► N19 ;  V2 ─► V3 ;  V7 ; V9 ; V4a ; V5 ; V6a ; X1a ;  D18 ─► {F7, X1b}
Onda N
  N1 (I0c, I2, I9) · N2 (F4, G2b) · N3 (T1a, I0c, I3b) · N4 (L6c) · N5a (G3a, G11) ─► N5b · N6 (L1b, G11) · N7a (I10, I17) · N7b (I0c, I10, I11b) ─► E1 · N15 (G11, V10a, N5b)
```

### 3.2 Ondas em três faixas

Três faixas, porque a máquina aguenta três builds com `CARGO_TARGET_DIR` próprio (§0, §6 "disco C cheio"): **A** motor e automação (sem `ui-egui`); **B** UI (dona de `chess_ui.rs`, `chess_collection.rs`, `chess_text_ui.rs`, `chess_*_ui.rs` e dos catálogos); **C** crates `chess` e `chessvision`. Uma faixa é dona dos seus arquivos durante a onda; quando um passo precisa de uma linha num arquivo de outra faixa (os quatro leitores do F1, por exemplo), entra depois do passo dono e rebaseia no mesmo dia. Dentro de uma célula, "→" é sequência. Dias = dias de agente pela coluna Esforço (S 0,5 · M 1–1,5 · L 2–3).

| Onda | Faixa A · motor/automação | Faixa B · UI | Faixa C · `chess`/`chessvision` | Dias (máx.) |
|---|---|---|---|---|
| **P** | **P (2)** · Passo 0: P1 feito; P2 · `parity/acrobat-features.toml` | — | — | 0,5 |
| **0** núcleo | **Q1a → G1 → G7a → E2** · `xtask/src/chess_gate.rs` (novo), `xtask/known-failures.toml`, `cos/src/writer.rs`, `engine/src/chess/{keys,corrections,epub}.rs`, `automation/src/{chess,chess_games}.rs`, `apps/pdfcraft/{build.rs,src/single_instance.rs,src/updates.rs}` | **G12 → G8** · `chess_autosave.rs`, `chess_collection.rs`, `chess_tools.rs` (lixeira), `chess_deleted.rs`, `editing.rs`; `engine/src/chess/collection.rs` (`show_or_add`) | **V1 → V8 → F1** · `chessvision/src/pieces/svg.rs`, `chessvision/src/models.rs`, `chess/src/{pgn.rs (novo), game/split.rs, game.rs}` + uma linha em cada leitor | 4,5 (as faixas B e C começam depois do Q1a) |
| **1** núcleo | **G2a → G11 (motor) → Q2 → Q1b** (o Q1b liga os benchs do Q2 na etapa 4) · `engine/src/chess/corrections.rs (+tests)`, `automation/src/chess_corrections.rs`, `engine/src/chess.rs` (`ChessCache` em mapa + LRU, `chess_cached`/`prime_chess_cache`), `chessvision/examples/*_bench.rs`, `xtask/src/chess_bench.rs`, `apps/pdfcraft-cli/src/main.rs` | **I15 (com I7a) → G11 (UI)** · `chess_ui.rs`, `docking.rs`, `widgets.rs`, `chess_text_ui.rs`, `chess_reference_ui.rs`, `chess_list.rs`, `chess_diagram_tab.rs`, catálogos | **F2a → F2b → F3 → Q4a → V6a** · `chess/src/game/{pgn_scan,pgn_read}.rs` (novos), `game.rs`, `game/{split,graft}.rs`, `fragment.rs`, `nag.rs`, `engine/src/chess/capture.rs`; `chess/src/{rules,placement}.rs`, `xtask/src/mutate.rs` | 5 |
| **2** | **T3 → G2b (depois do T1a, faixa C) → G6 → G3a (2)** · `platform/src/*`, `chessvision/src/{models,pieces,text_font}.rs`, `engine/src/chess/analysis.rs`, `apps/pdfcraft/src/single_instance.rs`; `engine/src/chess/{corrections,reference_store,store (novo)}.rs`, `engine/src/chess.rs`, `automation/src/chess_{corrections,reference}.rs`, `engine/tests/compat/` (novo) | **I18 → G2b (UI) → I1a → I11a** · `chess_tools.rs`, `chess_history.rs`, `canvas.rs`, `lib.rs` (preferências, I18); `chess_corrections_ui.rs`, `chess_list.rs`, `chess_ui.rs`, `chess_text_ui.rs`, `chess_export_ui.rs`, `chess_drafts.rs`, `widgets.rs` | **T1a → T2a → A9-L0a → A9-L0b** · `chess/src/fen.rs` + sítios; `engine/src/chess/span.rs`, `chessvision/src/doc{.rs,/build.rs,/games.rs,/reference.rs}`; os consumidores do `MoveInline` na aba Texto entram depois do I1a | 5,5 |
| **3** | **G4 → G5a → F4 → F7 → F5 (2) → F6 → X3 (depois do L6b, faixa C)** · `engine/src/chess/{reference_store,store}.rs`, `automation/src/chess_store_migrate.rs` (novo); `chess/src/{pgn,game,nag,san}.rs`, `chessvision/src/doc/pgn.rs`, `automation/src/chess_pgn.rs`; `chessvision/src/doc/html.rs`, `engine/src/chess/{epub,office}.rs` | **G10 → I1b → T2b → T1b → I2 (2)** · `chess_corrections_ui.rs`, `chess_drafts.rs`, `chess_redraw.rs`, `chess_diagram_keys.rs`, `chess_reference_ui.rs`, `chess_ui.rs`, `chess_editor.rs`, `chess_board_ui.rs`, `chess_text_ui.rs`; `engine/src/chess.rs` (`step`) | **L2 → L1a → L1b → L3 → L5 → L6a → L6b → L6c → L7 (+ G7b) → L8 → X1a** · `chessvision/src/doc/{build,games,reference}.rs`, `chessvision/src/chess_text/*`, `chessvision/src/doc/{html,markdown}.rs`, `chess/src/{describe,recognize/side}.rs`, `export/src/zip.rs` | 9 |
| **4** | **V2 → V3 → V7 (2) → V9 → V10a → V4a → V5** · `chessvision/src/{glyphs*,detect/*,imageops*,cipher.rs,text_font.rs,pieces/import.rs}`, `engine/src/chess/text_ocr.rs`, `engine/src/chess.rs` | **I0a → I0b → I0c → I4 → I8 → I3a** · `docking.rs`, `chess_ui.rs`, `chess_board_ui.rs`, `chess_window.rs`, `lib.rs` (modo Ler), `chess_list.rs`, `chess_diagram_tab.rs`, `chess_autosave.rs`, `chess_collection.rs` | **Q3a → Q3b → Q5 → Q4b → X1b → N4** · `xtask/src/fuzz.rs`, proptests; `ui-egui/src/control.rs`; `ui-egui/tests/*`; `engine/src/chess/office.rs`, `export/src/flow.rs`; `chess/src/mate.rs` (novo) | 8 |
| **5** | **I3b → G3b → G2c → G3c + V4b → G5b → I13 → I5 (motor)** · `engine/src/chess.rs`, `page_key.rs`, `chessvision/src/doc/reference.rs` (índice compacto), `engine/src/chess/store.rs`, `chess_autosave.rs` (gravador), `chess/src/game/structure.rs`; `engine/src/chess/analysis.rs`; os enums de razão | **I9 → I10 → I17 (2) → N7a → I11b → I14 (2) → I16 → X2 → I6 → I7b → V6b → V10b** · `chess_text_ui.rs`, `chess_tools.rs`, `chess_board_ui.rs`, `chess_study_ui.rs`, `widgets.rs`, todos os `chess_*.rs` (o I17 vai sozinho), catálogos, `engine/src/commands.rs`, `chrome.rs`, `chess_export_ui.rs` | **L4 → L9 (2) → G9 → N19** (o G9 espera o I10, faixa B) · `chessvision/src/doc/games.rs`, `chessvision/examples/walk_bench.rs`; `chess/src/lichess.rs`, `engine/src/chess/links.rs` + 3 chamadores da UI (depois do I10) | 12 |
| **6** | **N5a → N5b → N8 → N6** | **N1 → N3 → N7b → E1** | **N2 → N9 → N10** → N11, N12, N15–N18 conforme D20–D23 | 4,5 |

**Fim de cada onda:** o E0 (o `dist/app` refeito, o anterior completo, as notas da versão).

**Caminho crítico:** a faixa B (≈ 38 dias de agente). A UI é serial porque `chess_ui.rs`, `chess_text_ui.rs` e `chess_collection.rs` são tocados por ≈ 12, ≈ 10 e ≈ 7 passos; uma quarta ou quinta sessão não encurta o plano. O I1a pode, sem mudar comportamento, separar `chess_ui.rs` em resultados/lista, jobs e revelação, para que I2, I4 e I0 deixem de disputar um arquivo — é o único jeito de a faixa B deixar de ser o caminho crítico.

**Estimativa:** ≈ 120 PRs até a onda 6 (119 pela soma da coluna PRs da ficha §3.4, com E1 e E2, sem N11–N18, que ganham ficha com D20–D23, nem as notas do E0) — ≈ 120 dias de agente, ≈ 24 semanas de um agente só; com três faixas, ≈ 10 semanas de parede, ou ≈ 12–13 com revisão, rebase e as sessões do usuário (D18, D26). O núcleo (§3.0) é o que vale fazer mesmo se o resto parar.

**Arquivos quentes** (risco de colisão; ver §6):

| Arquivo (linhas) | Passos que o tocam |
|---|---|
| `ui-egui/src/chess_ui.rs` (1.700; 1.706 no `main`) | I1, I15, I0, I2, I4, I3, I11, T1, T2, G2b, V10b, N1 (≈ 12) |
| `ui-egui/src/chess_text_ui.rs` (1.068) | G11, I1, I2, I4, I5, I9, V4, I15, T2, A9-L0 (≈ 10) |
| `engine/src/chess.rs` (1.280) | G11, G2b, G6, V10, I2, I3, Q4, L6, G9 (≈ 9) |
| `chessvision/src/doc/games.rs` (2.069) | A9-L0, L2, L4, L5, L6, L7, L9, T1, G7b (≈ 9) |
| `ui-egui/src/chess_collection.rs` (3.179) | F1, G8, I1, I3, T2, G3b, F2 (≈ 7) |
| `chess/src/game.rs` (2.351) | F1, F2, F3, F4, F5, T1 (6) |
| `engine/src/chess/corrections.rs` (1.085) | G1, G2, G3, G7, G10, I1 (6) |

### 3.3 Passos

Cada passo: **Tarefas** · **Verificação** (o teste que falha antes) · **Esforço** (S < 150 linhas, M < 600, L acima), e a ficha em §3.4. **Padrões** (valem quando o passo não diz outra coisa): *Contexto* = as evidências (Anexo A) dos achados listados + a seção do §2 citada + o relatório `analise-9/<A|R>n.md` do achado; *Saída* = a Verificação verde no `chess-gate`, a linha do §5 que o passo move, a nota do `parity/` e, se o passo muda algo visível, a seção de `docs/chess.md` e uma linha em `docs/chess/ajuda/PENDENTE.md` com o capítulo da ajuda que ficou velho (E1); *Reversão* = revert do PR — e é **obrigatória e própria** em todo passo com "formato gravado" na ficha. "Achados" = o que fecha. Os passos estão agrupados por tema; a ordem de execução é a da §3.2.

#### Onda Q — portões e testes

**A9-Q1 — Portão local único `cargo xtask chess-gate`.** Achados A7-03, A7-04, A7-05, A7-06, A7-14. Dois PRs: **Q1a** (onda 0: executor das etapas 0–3, `known-failures.toml`, `PDFCRAFT_REQUIRE_MODELS`) e **Q1b** (onda 1: construtores do A7-14, `test_models()`/`test_engine()`, etapas 4, 4b e 5, a **jornada de ouro** na etapa 3 — num livro sintético: detectar → corrigir o diagrama 2 → Confirmar → Salvar como → reabrir → exportar EPUB → a FEN corrigida no EPUB, com um store no formato de `cc2d484c` na pasta; pega a soma de G3a, G5a, I1 e G11, que nenhum teste de passo pega —, e a primeira base de desempenho — `cargo xtask chess-gate --baseline` compila o `main` num worktree e `CARGO_TARGET_DIR` próprios, `C:/Python-Chess2/cargo-targets/gate/`, roda os `perf_*` e grava `perf-baseline.json`; ≈ 4 GB, um build release por vez).
- *Contexto:* `xtask/src/{main,gates,parity}.rs`, `automation/src/file_access.rs`, `ui-egui/src/i18n/{mod,catalog}.rs`, `analise-9/A7.md` (tabela de etapas).
- *Tarefas:* (1) `xtask/src/chess_gate.rs`: lê `git diff --name-only $(git merge-base HEAD origin/main)` e roda, parando na 1ª falha: etapa 0 CRLF (`git ls-files --eol`) + `fmt --check`; etapa 1 varreduras textuais (`tl!` × 18 catálogos + SHA-256, guarda `file_access` exigindo `resolve(` em código, não em comentário, `xtask parity`, leitura do TOML também por um parser TOML 1.0, `xtask layers`, `xtask assets`); etapa 2 clippy + testes de `pdfcraft-chess` e `pdfcraft-chessvision` + `cargo check --target wasm32-unknown-unknown` dos dois; etapa 3 (se tocou L6+) `cargo check -p pdfcraft-cli -p pdfcraft`, `cargo check --target wasm32-unknown-unknown -p pdfcraft-engine -p pdfcraft-automation`, engine filtrado em `chess`, `automation` inteiro `--no-fail-fast`, `ui-egui --test chess --test 'chess_*'`; etapa 4 (se tocou o walker/visão) benchs de Q2; etapa 4b "desempenho" (se tocou walker, visão, stores ou `chess_autosave`) `cargo test --release -p pdfcraft-chess -p pdfcraft-chessvision -- --ignored perf_` e, se tocou L6+, os `perf_` do `pdfcraft-engine` filtrados em `chess` (§5 "Como medir"); etapa 5 (`--full`) `chess-bench` 38 livros e fuzz. O "etapas 0–2 < 5 min" é medido com `CARGO_INCREMENTAL=0` (que torna toda recompilação do crate tocado completa); um alvo do app em release ocupa 4,1 GB, e cada sessão com `CARGO_TARGET_DIR` próprio multiplica isso. (2) As duas varreduras viram módulos sem dependências (`#[path]` incluído pelo `xtask` e pelo crate, ou um crate L0 novo); o `chess-gate` e o teste fino do crate chamam o mesmo código; nenhum crate de produto depende do `xtask`. (3) `xtask/known-failures.toml` (`test`, `reason`, `since`, `kind`); o portão falha em falha fora da lista **e** em teste da lista que voltou a passar. (4) `PDFCRAFT_REQUIRE_MODELS=1` transforma o pulo dos testes com modelos/motor em falha; o portão passa os modelos e o motor só pelas variáveis de teste (`PDFCRAFT_CHESS_TEST_MODELS`/`_ENGINE`) e roda os testes com `PDFCRAFT_MODELS` e `PDFCRAFT_CHESS_ENGINE` **tirados** do ambiente, e os seis testes de `ui-egui/tests/chess.rs` que hoje saem com `return` quando há modelo ou motor instalado (linhas 696, 2018, 3912, 4011, 4159, 4248 — entre eles `the_review_journeys_take_the_gestures_the_plan_set`, as jornadas do A8) passam a fixar "sem modelo"/"sem motor" na própria sessão em vez de sair; o portão recebe `--models DIR` e `--engine EXE` (ou as variáveis com caminho absoluto; os worktrees não têm `dist/app`) e falha com mensagem clara se faltarem; um helper `test_models()` substitui as seis cópias de `PDFCRAFT_CHESS_TEST_MODELS`, e um `test_engine()` a de `PDFCRAFT_CHESS_TEST_ENGINE`. (5) Construtores ou `..Default::default()` nos literais de `DiagramReplacement`, `OcrOptions`, `ChessExportOptions`, `LinkSettings` fora do crate dono. (6) Resumo em `target/chess-gate.json` para o PR citar.
- *Verificação:* sabotagens, cada uma vista falhando na etapa certa: reintroduzir o #185 (campo novo sem atualizar a CLI) → etapa 3; `"` sem escape numa nota do parity → etapa 1; um `std::fs::read` sem `resolve` em `automation/src/chess.rs` → etapa 1 em < 30 s; um arquivo CRLF → etapa 0; uma string `tl!` nova sem catálogo → etapa 1.
- *Saída:* etapas 0–2 em < 5 min incremental; `docs/chess/blueprint.md` §4 e `CLAUDE.md` apontam para o comando, e o §4 do blueprint passa a dizer **18** catálogos (hoje diz 3).
- *Reversão:* só arquivos novos em `xtask` + o teste fino; revert.
- *Esforço:* M.

**A9-Q2 — Benchs-portão pela verdade.** Achados A7-09, A2-03 (métrica).
- *Contexto:* `chessvision/examples/{walk_bench,reference_bench}.rs`, `xtask/src/chess_bench.rs` (casamento por página, posição e texto).
- *Tarefas:* `walk_bench` e `reference_bench` ganham `--baseline FILE --allow-lost N` com o casamento de `chess_bench.rs`; `walk_bench --reference=livro.pgn` reporta confirmed/conflict/absent por `reference::align`; o `chess-gate` etapa 4 roda os dois contra bases guardadas em `cargo-targets/gate/` (geradas do `main` do dia) e `ocr_bench --gate`.
- *Verificação:* reverter #153 (colchete dentro de palavra) e ver `walk_bench --baseline` falhar nas páginas de Pinski.
- *Saída:* números de base registrados em `docs/chess.md` §Measurements (válidos, válidos a partir de diagrama, confirmados pelo PGN do livro).
- *Reversão:* revert. *Esforço:* S–M.

**A9-Q3 — Fuzz e proptest onde o xadrez é frágil.** Achados A7-08, A7-11, R7-01 (corpus). Dois PRs: **Q3a** (fuzz pela CLI, modelos hostis construídos) e **Q3b** (proptests).
- *Tarefas:* `xtask fuzz --cli PATH --models DIR` (reusar o `dist/app`; o filho roda `chess_document_with` com `OcrMode::All`); mutador de texto de lances nos `Tj` da semente de captura; no `check-one --chess`, passar o texto da página por `split_pgn` → `from_pgn_report` e por `ReferenceIndex::from_pgn`; `VOCAB` da proptest do walker com `;`, `a)`, `b)`, `…`, `...`, negrito, figurinas, "Position after", "Exercise 4" e `ocr: bool`; um corpus **construído** de modelos ONNX hostis (`ConstantOfShape`, `Expand`, `Tile`, `Range`, `Resize`, `Pad` com forma grande), além da mutação de bytes — as 4.600 mutações do A3 não podiam achar o R7-01; proptests novos para `split_pgn` (N partidas → N pedaços, concatenação preservada), `recognize::decode`, `pieces::images::march`, `detect::contour`, escape RTF/DOCX, `uci::parse_info`.
- *Verificação:* uma semente com `; {` faz o fuzz acusar a perda de A1-05 antes de F2; uma mutação manual no ramo `separators` passa a ser morta pela proptest.
- *Esforço:* M.

**A9-Q4 — Higiene dos testes.** Achados A7-07, A7-15, A7-16, A7-17, A7-18, R2-04. Dois PRs: **Q4a** (onda 1, crate `chess`: mutantes do `rules.rs`, `Square`, `xtask mutate`) e **Q4b** (onda 4, testes de UI: esperas, mutex de GPU, 703 páginas, `static MODELS`, testes vazios).
- *Tarefas:* dois casos de reis adjacentes (e4/e5, e4/f5) que matam os mutantes de `rules.rs:215`; `Square::new`/`offset` totais (`.then(||…)`) com proptest sobre todo `(u8, u8)`; `cargo xtask mutate <arquivo> [--lines]` (o invólucro de Windows do `mutator`: cópias em vez de symlinks, `taskkill /T`); esperas por evento em vez de relógio nos testes de UI (`recv_timeout(60 s)`); `tests/common/gpu.rs` com o mutex único para capturas e pixels (cinco arquivos); teste de 703 páginas → `#[ignore]` + versão de 48 páginas + teste de despejo com `MAX_CACHED_PAGES + 1`; o `static MODELS` do motor (`engine/src/chess.rs:402`) é do processo, e `ui-egui/tests/chess_models.rs:155` instala nele modelos inválidos enquanto outros testes do mesmo binário rodam — dar ao teste um cache próprio ou serializá-lo; os quatro testes vazios (`progress_can_stop_a_scan`, `a_document_that_forbids_copying_is_not_scanned`, `a_carry_needs_the_document`, `textured_themes_draw_the_board`) passam a afirmar o resultado visível.
- *Verificação:* cada teste reescrito visto falhando com a sabotagem que hoje passa; `mutate rules.rs` com 42/42.
- *Esforço:* S–M.

**A9-Q5 — O canal de controle alcança o teclado do xadrez.** Achado A6-19.
- *Contexto:* `ui-egui/src/control.rs:197-201`, `lib.rs:3172-3178`, `chess_tools.rs` (`take_scope_keys`).
- *Tarefas:* os eventos injetados passam pelo mesmo filtro de escopo (`take_scope_keys`) que o `raw_input_hook` aplica; o `ui inspect` passa a serializar `live`, `modal`, `expanded`, `has_popup`, `active_descendant` e `bounds` dos filhos (`control.rs:636-672`), para conferir a matriz §2.8.3 no app instalado; roteiro das jornadas de teclado do A8 P10 (Diagrama: setas, letras, Enter, PgUp/PgDn, Ctrl+Z, Esc; lista; tabuleiro) em `tests/control.rs`.
- *Verificação:* `ui key ArrowRight` move a casa do tabuleiro; falha antes.
- *Esforço:* S–M.

#### Onda T — tipos que servem a todas as ondas

Vêm logo depois do Q1 (ub-mode: *do first whatever helps every later phase*). Cada um troca uma decisão espalhada por um tipo, um sítio por commit, e tem uma **contagem de sítios que vai a zero** como saída.

**A9-T1 — `Fen` e as chaves de posição no `pdfcraft_chess`.** Achado R3-03 (o `position_key` do N3 sobe para cá). Dois PRs: **T1a** (`Fen` e os métodos no `pdfcraft_chess`, sítios de `chess`/`chessvision`) e **T1b** (sítios do motor, da automação e da UI).
- *Contexto:* `chess/src/fen.rs`; os ≥ 12 sítios de R3-03; `chessvision/src/doc/reference.rs:295-303` (`position_key`).
- *Tarefas:* `Fen` validada uma vez na entrada (parse → tipo; dentro, confiar); as três equivalências de hoje viram métodos nomeados — `placement()`, `book_key()` (`BookKey { placement, side }`, com o caminho "só colocação" quando o lado foi suposto, que `has_position` já tem) e `full()` (a "mesma posição" da FIDE, para repetição e cache do motor) — e cada sítio diz qual usa; `Game::fen_at` devolve `Fen`; a UI deixa de reler a FEN para ter as peças. **Sem custo nos laços quentes:** `Fen::from_board` infalível (sem reparse) para `Game::fen_at` e para o `ReferenceIndex::add_game`, que chama `fen_at` por nó (até 1 milhão); `BookKey` como hash de 64 ou 128 bits da colocação e da vez, não `String`.
- *Verificação:* as equivalências como propriedades (proptest); `grep` de fatias de FEN (`split_whitespace().next()` e afins) fora de `fen.rs` → 0; wasm.
- *Esforço:* M (vários crates; um sítio por commit).

**A9-T2 — Um tipo por endereço e um rótulo de página.** Achados R3-09 (endereços), R3-08, R3-04 (parte do tipo). Dois PRs: **T2a** (`MoveAt { page: u32, block: u32, inline: u32 }` desce para `chessvision::doc` e substitui o `InlineAt`, com a mesma forma serde; `engine::chess::span::MoveAt` vira reexportação; `page_label` no motor e nas exportações) e **T2b** (`DiagramAt` e o rótulo na UI). Os campos soltos da `Decision` viram `#[serde(flatten)] at: MoveAt` no G4, sem mudar o JSON gravado (hoje `MoveAt.page: usize`, `InlineAt.page: u32`, `Decision.page: usize`).
- *Tarefas:* um tipo de endereço de lance no lugar de `MoveAt` (`span.rs:38`), `InlineAt` (`reference.rs:332`) e dos campos soltos de `Decision`; `DiagramAt { page, index }` no lugar das tuplas `(usize, usize)` (14 em `chess_ui.rs`; `ListState`, `review_done`, `picked`), com `DocId` na UI; `page_label(doc, index) -> PageLabel { printed, physical }` no motor (com o folio do L1 como recuo quando chegar) usada pelas exportações e pela UI inteira.
- *Verificação:* nenhuma tupla `(usize, usize)` de diagrama em `chess_*.rs`; num livro com `/PageLabels`, lista, aba Diagrama, aba Texto, editor clássico e nome da partida dizem o mesmo rótulo (falha antes: "13" × "25").
- *Esforço:* M.
- *Reversão:* um build revertido lê a mesma `Decision` (o JSON não muda: `#[serde(flatten)]`); revert.

**A9-T3 — `AppDirs` resolvido em L7.** Achado R3-07.
- *Tarefas:* `AppDirs { install, data, portable }` em `platform` (L7), passado como parâmetro às funções do motor e da visão (modelos, peças, fontes, motor UCI); nenhum `current_exe` abaixo de L7; base de G5a, G6 e I13 (o G7a, na onda 0, recusa `chess_pieces_import` sem `into` sob `--automation-write-root` (sob `--root` a recusa já existe, `automation/src/chess.rs:884`) sem esperar o T3; o T3 só troca a origem do padrão).
- *Verificação:* `grep current_exe` em `chessvision`/`engine` → 0; os testes de modelos/peças/fontes recebem a pasta por parâmetro.
- *Esforço:* S–M.

#### Onda G — guarda dos dados

**A9-G1 — Identidade do documento.** Achados A4-03, R3-09 (identidade). §2.2.
- *Contexto:* `cos/src/writer.rs:150-160,340-362,556-577`; `engine/src/chess/keys.rs`; `corrections.rs:203-218,503-513` (`DocRecord::matches`, `file_name`); `epub.rs:94-97`.
- *Tarefas:* `ensure_id` sobre todo o corpo serializado; **uma** regra de identidade: `DocRecord::matches` delega a `DocKeys::matches` (caminho comparado pelo hash) e o veto do `id0` entra uma vez (a `StoredKeys` da coleção vira `DocRecord` em G3); `KNOWN_CONSTANT_ID` tratado como ausente; um arquivo de store chaveado pelo `/ID` constante que já mistura dois livros é separado no `update` ou na migração — nunca no `load`, que não grava: as entradas cujas `page_fp` estão no documento aberto vão para o arquivo dele (gravado primeiro; depois o compartilhado é regravado sem elas; idempotente por `uid`), e as outras ficam no arquivo compartilhado, listadas como "de outro livro", até o outro livro abrir e levar as suas (nunca apagadas); um build revertido não vê as entradas já separadas, e o PR diz isso; identificador do EPUB com o intervalo quando parcial; nota para o upstream em `docs/` (D24).
- *Verificação:* (1) `extract` de páginas diferentes → `/ID` diferentes (falha antes); (2) correção gravada para `a.pdf` não aparece em `b.pdf` com outro `id0` mesmo com mesmo nome e tamanho, e os três stores casam ou recusam o mesmo par de chaves; (3) um arquivo com o `/ID` constante casa pelo caminho e não pelo `id0`; (4) dois EPUBs parciais com identificadores diferentes.
- *Saída:* parity `edit.chess-diagram-corrections`, `chess.book-reference-pgn`, `export.chess-epub` com a nota.
- *Reversão:* revert (o `/ID` de arquivos já salvos não muda). *Esforço:* S.

**A9-G2 — Onde está a página agora, e quem fica com o diagrama.** Achados A4-01, A4-02, R1-01, R3-06, R3-04 (âncoras das partidas), R8-04. §2.4. Três PRs: **G2a** (onda 1: `locate`, `Orphan`, impressão vence índice, `set` substitui, formato v2 com o teste de compatibilidade; S–M) · **G2b** (onda 2: `assign` O(D + C), mapa de impressões no `ChessCache`, a UI lendo `Corrected.applied`, `perf_assign`; M) · **G2c** (onda 5, depois do G3b: as âncoras `NodeSource`/`Source` das partidas, que mudam o arquivo das partidas).
- *Contexto:* `engine/src/chess/corrections.rs:380-470,640-712`; `corrections/tests.rs:120-140`; roteiros `analise-9/` de A4 (`s2.json`, `s15.json`) e de R1 (`s_a401b.json`).
- *Tarefas:* formato das correções sobe para a versão 2 (§2.10); `locate` único com `prev_fp`/`next_fp` e `Orphan`; `assign` puro como **a** regra de atribuição (impressão digital vence índice; `at`/`rev` desempata só entre iguais; uma correção por diagrama), usado por `apply`; a UI passa a ler `Corrected.applied` do motor em vez de chamar `find` por diagrama, e deixa de guardar sua cópia das correções (dono único: a sessão); `set` localiza a viva e a substitui; motivo de `unmatched` diz qual correção tomou o diagrama; `chess_correction_list` mostra `orphans`; o teste que consagrava A4-01 passa a exigir `Orphan`; as âncoras das partidas (`NodeSource`/`Source`) guardam a `page_fp` e são relocalizadas pelo mesmo `locate` no `chess_carry`.
- *Verificação:* testes de `CorrectionFile`, cada um visto falhando: (1) apagar a página com correção → a nova página k fica sem correção; (2) correções nas págs. k e k+1 com a mesma caixa, apagar a k → `apply` dá a da k+1 e a da k fica `Orphan` (R1-01); (3) inserir página e recorrigir → `live().count() == 1` e `apply` dá a última; (4) a marca da UI e a exportação dão a mesma posição para o mesmo diagrama; (5) inserir uma página → "Mostrar no PDF" de um lance da coleção vai à página nova; (6) `perf_assign` com 1.104 pp., 5.334 diagramas e 5.334 correções, depois de apagar a pág. 1, conta ≤ 2·(D + C) comparações de impressão (hoje D·C·P: 1,7 s em 748 pp./999 e ≈ 73 s em 1.104 pp./5.334). No mesmo PR, já que o formato sobe para v2, compactar: `orig` omitido quando igual a `fen` e as impressões numa tabela por página, em vez de repetidas em cada entrada.
- *Esforço:* M (sobe de S–M com o `assign` e as âncoras).
- *Saída:* correção nunca aplicada a outro diagrama; parity atualizado. *Reversão:* um build revertido abre os arquivos v2 só para leitura (não os regrava sem as vizinhas); revert.

**A9-G3 — `DocStore` no motor, para os quatro tipos.** Achados A4-06, A4-07, A4-13, A4-16, A4-17, A4-20, A7-02, R1-03, R2-03, R3-05, R7-03 (no G3b). §2.3, §2.10. **G3a** (onda 2, dois PRs: o genérico + correções; o PGN do livro + ferramentas `read_only` + `.flock` + `persist_policy`; aqui o formato `reference` sobe para v2 e nasce `tests/compat/<kind>_vN.rs`) · **G3b** (onda 5, depois do I3b: partidas) · **G3c** (com o V4b: cifra). A política de versão do `.games` é o passo G12 (onda 0).
- *Contexto:* `corrections.rs`, `reference_store.rs`, `automation/src/{chess_corrections,chess_reference}.rs`, `platform/src/staging.rs`.
- *Tarefas:* `store.rs` com `StoreKind` (com `merge`)/`StoreBackend`/`DocStore`; correções e PGN do livro viram `impl StoreKind` (sai a cópia); depois a coleção (`GameRecord` em tipos de L0/L6, `StoredKeys` → `DocRecord`, sem pôr de lado arquivos de outros documentos) e a cifra; um orçamento web só; `MemBackend` e `DirBackend`; nome `<hash>.<kind>.json` com leitura do nome antigo; `load` só lê; validação por entrada (inclusive `page ≤ MAX_PAGES`, R2-03; respostas com `saturating_add(1)`); trava `<kind>.flock` com a de transição (§2.3); limpeza de temporários; `name_for` com filtro e cabeçalho; ferramentas de leitura com `Access::Read`; `set_aside`/`skipped` em toda resposta de escrita; `MAX_ENTRIES` coerente com `MAX_FILE_BYTES` e compactação de lápides antigas; teste estrutural das ferramentas `read_only` (R1-03).
- *Verificação:* o roteiro de A4-07/A7-02 (mesma pasta para os dois) termina com `kept: true` e nenhum `.bad`; um arquivo com 1 entrada ruim entre 16 mantém 15; `chess_diagram_states` roda com MCP só-leitura; toda ferramenta `read_only` deixa a pasta byte a byte igual; duas threads com trava "velha" artificial nunca entram juntas; o teste concorrente atual (2 × 24 escritas) continua sem perda sobre os dois backends **e** com um escritor do protocolo antigo (fixture) contra um do novo na mesma pasta; arquivo com `page = usize::MAX` → entrada descartada e contada; uma pasta com um `.games` v1 de outro livro e um `<hash>.games.json` v2 → o `find` pelo código v1 congelado (§2.10) não renomeia nada; a CLI lê as partidas que o app gravou; app com 3 partidas na memória + importação de 2 no mesmo store → depois do autosave, o arquivo tem 5 (falha antes: 3); duas abas web, cada uma acrescenta uma partida a um livro diferente → as duas ficam; nenhum `C:\Users` num `.games.json`; um PDF cifrado com a política "Ask" → nenhum arquivo antes do "Sim"; `cargo xtask layers` e `cargo check --target wasm32-unknown-unknown -p pdfcraft-engine` verdes.
- *Saída:* `dryer` sem pares ≥ 0,9 entre os stores; um só formato de identidade nos quatro. *Esforço:* M + M. *Reversão:* o G3a grava só o nome novo; a cada carga, se o nome antigo mudou desde a última vez (tamanho + mtime em `migrated.json`), ele é mesclado por `K::merge` (a regra da migração do §2.3) — duas fontes de verdade nunca; um build revertido não vê o que foi gravado no nome novo, e o PR diz isso.

**A9-G4 — PGN do livro idempotente.** Achados A4-08, A5-08. Decisão D25.
- *Tarefas:* a `Decision` passa a guardar o lance escolhido do PGN (`pgn_san`, `#[serde(default)]`; hoje `Choice::Pgn` não diz qual era, R6-10); `set_bytes` mantém as decisões quando o hash é o mesmo (bug); com outro PGN, conforme a D25 — e uma decisão só continua valendo se o PGN novo ainda tiver aquele lance: manter as decisões e devolver `decisions_unmatched` (a UI diz quantas não casam mais), ou perguntar antes de descartar.
- *Verificação:* `set`, `decide` ×2, `set` (mesmo arquivo) → 2 decisões (falha antes).
- *Esforço:* S.
- *Reversão:* um build revertido lê `reference` v2 só para leitura pela versão e ignora `pgn_san`/`bytes_hash`; revert.

**A9-G5 — Um lugar para os stores, cópia e troca de máquina.** Achados A4-12, A4-21, A8-19. Decisão D17. Dois PRs: **G5a** (onda 3: local único e migração que mescla, para correções e PGN do livro) e **G5b** (onda 5, depois do G3b: o pacote de revisão com as partidas).
- *Tarefas:* local padrão na pasta de dados do app (`AppDirs.data`, T3), usado por app e CLI para os quatro stores; migração que mescla (§2.3: `DocStore::update` com `K::merge`, `migrated.json`, pode rodar de novo, a origem nunca é movida nem apagada) de `Recovery/chess/*` e da `.pdfcraft/` da raiz em uso, pela ferramenta `chess_store_migrate {from}`; `--root` restringe; `chess_review_pack_export/import` (correções + decisões + partidas, sem o texto do PGN; importar mescla por `K::merge` de cada tipo e devolve o que não casou — possível porque, depois do G3, as partidas estão num store do motor que a `automation` alcança); comando no menu Xadrez ▸ "Exportar revisão do livro…".
- *Verificação:* correção feita pelo store do app é lida por `chess_document` sem `store`; exportar → importar noutra pasta → mesmas contagens em `chess_diagram_states`; exportar e importar **duas vezes** → as mesmas contagens (falha antes: partidas duplicadas); um pacote com uma correção `page = usize::MAX` → o resto importa e 1 é contada como descartada; nenhum `C:\Users` no pacote; corrigir no build antigo depois da migração → o novo vê a correção no próximo início; o mesmo livro nas duas origens → as entradas das duas no destino; `Stop-Process` no meio da migração e reabrir → termina sem duplicar.
- *Reversão:* o build revertido não vê o que se fez depois da migração — dizer no PR; `chess_store_migrate {from: data, to: recovery}` leva de volta.
- *Esforço:* M.

**A9-G6 — Chave do modelo.** Achado A4-14 (o A6-27 foi para o E2). (A parte web, A4-15, vai com o `UiStore` de I1.)
- *Tarefas:* `ModelKey::Folder(path, sha256)` do `.onnx` e do `.json`, calculado **na carga** e guardado no `ChessModels` (nunca em `ModelKey::of`, que roda a cada varredura, `carry` e consulta ao cache, `engine/src/chess.rs:512,1040,1121`) (~10 ms para 11 MB, uma vez por carga; o `len` não distingue modelos da mesma arquitetura — o real e uma cópia com todos os pesos mudados têm 2.793.731 bytes cada).
- *Verificação:* duas cargas diferentes na mesma pasta não respondem do cache.
- *Esforço:* S.

**A9-G7 — Confinamento, tetos e saídas nas ferramentas.** Dois PRs: **G7a** (onda 0, só `automation`: `chess_pieces_import`, `read_capped` público, sobrescrita, saídas distintas, caminhos relativos, e a recusa do `chess_games` com `pgn_stopped` do G8) e **G7b** (`PageSet` no motor e no walker, com o L7). Achados A4-04, A4-05 (recusa com `pgn_stopped`, salvo `allow_truncate`), A4-10, A4-11, A4-22, A2-16, R7-04, R7-06. Também: as saídas de uma mesma chamada são distintas entre si pelo caminho canônico (hoje `chess_export_markdown {out:"book.md", cipher_out:"book.md"}` responde "exportado" e o arquivo tem o JSON da cifra), senão `InvalidArgs`; as respostas trazem caminhos relativos à raiz, e fora dela só o nome e `"outside_root": true` (hoje `chess_status` e `chess_pieces_import` devolvem `\\?\C:\Users\…` a um agente confinado).
- *Tarefas:* `chess_pieces_import` sem `into` recusado quando há `root`/`roots` (ou destino padrão por `check_roots(.., Write)`); varrer as outras ferramentas com "padrão ao lado do executável"; o `corrections::read_capped` que já existe (`corrections.rs:1057-1064`, `pub(super)`) é publicado ou descido para um módulo comum e usado por `cipher_in`, `chess_games` e onde mais houver `read_to_string` — nenhuma segunda cópia; recusar alvo de exportação que é o caminho de um documento aberto ou uma entrada da mesma chamada, salvo `overwrite: true`; `PageSet` (ordenado, sem repetição, ≤ páginas do documento) no **motor**, recebido por `chess_document_with` e pelo walker — a UI, as exportações e a CLI chamam a mesma função; a ferramenta só converte na entrada.
- *Verificação:* os roteiros de A4 (MCP de `chess_pieces_import`, `cipher_in` de `MAX+1`, `chess_export_markdown out:"doc.pdf"`) e o `pages:[2,1]`/`[1,1]` de A2 viram testes de `automation` que falham antes.
- *Esforço:* S.

**A9-G12 — Versão do `.games` sem pôr de lado outros livros** (onda 0, S). Achado R3-05.
- *Tarefas:* no store de partidas de hoje (`ui-egui/src/chess_autosave.rs:296-298,391,407-447,488-492`), um arquivo de versão maior abre só para leitura, e o `find` nunca renomeia o arquivo de **outro** documento; é a condição para qualquer mudança no formato das partidas (I3a, G3b).
- *Verificação:* uma pasta com um `.games` v1 de outro livro, um `<hash>.games.json` v2 e um `.games` v2 artificial → procurar o documento A não renomeia nada (falha antes: `.bad`).
- *Esforço:* S.
- *Reversão:* não reverter depois do G3b (o revert volta a pôr de lado `.games` de outros livros); antes dele, revert.

**A9-G8 — Coleção e lixeira sem perda.** Achados A5-01, A5-07, R1-02, A5-16.
- *Tarefas:* partida aberta de um diagrama e **intocada** é efêmera (abrir outro diagrama a substitui; não conta para o teto nem é gravada); `add_games` recusa no ato acima do teto, com aviso; `ChessGames::unsaved()` entra no `first_dirty` de Sair/Fechar ("Partidas de xadrez não salvas: Salvar todas como PGN…"); a lixeira passa a ser alimentada pelo próprio passo de edição (`BoardState::edit` devolve os nós removidos), não pelo desenho do trilho; `GameList::show_or_add(entry, key)` no motor decide "mesma partida intocada" com chave barata; na interface, "Salvar todas como PGN…" diz "N partidas foram lidas só até um lance impossível; o resto delas não será gravado" com Cancelar (R1-02) — a correção de raiz é F3.
- *Verificação:* coleção com 256 + diagrama novo intocado → substitui; `request_quit` com `blocked == TooBig` → pergunta; `load_pgn` de partida mais longa → item na lixeira; `MoveEdit::Delete` sem o trilho → item; o roteiro `s13.json` é recusado; kittest: importar PGN com `Bxh7` impossível e "Salvar todas" → aviso presente.
- *Esforço:* M.

**A9-G11 — Ler o livro uma vez por sessão** (onda 1, núcleo; antes do N5). Achado R8-01.
- *Tarefas:* `start_chess_job` (`chess_text_ui.rs:768-799`) passa ao job as `Arc<ChessPage>` que a sessão principal tem para o estado atual (funções **novas** `Session::chess_cached(id, pages) -> Vec<Arc<ChessPage>>` e `Session::prime_chess_cache(id, pages)`; hoje só há `store_chess_scan`, `engine/src/chess.rs:994`); o `JobDone` devolve o `ChessScan` do que o job leu, e o `poll_chess_job` o grava com `store_chess_scan`. O N5 passa a resolver só a **reabertura**. O `ChessCache` vira `HashMap<(DocId, page), …>` (hoje `get`/`put` fazem `retain` linear sobre as 2.048 entradas a cada página) com LRU **por documento**, para que dois livros grandes abertos juntos (1.481 + 1.104 pp.) não se despejem um ao outro.
- *Verificação:* com o contador `ChessCache.scanned` (já existe "for tests", `engine/src/chess.rs:240`): Detectar no livro → Exportar EPUB dá 0 páginas varridas no job (falha antes: 748); exportar duas vezes dá 0 na segunda.
- *Esforço:* S–M.

**A9-G9 — Links do lichess convergentes e com a orientação certa.** Achados A4-09, R3-02 (URL).
- *Tarefas:* o `pdfcraft_chess::lichess::analysis_url(&Position, Color)` que já existe recebe a orientação do `DiagramReading`, e a regra "só legal e não reparada" sobe para um lugar do motor; os dois usados por editor, tabuleiro, lista, links no PDF e `confirm` (hoje 8 lugares; os links gravados no PDF saem sempre do lado das brancas, `links.rs:78-84`); os retângulos de links e rótulos existentes entram em `taken`; um diagrama que já tem link lichess na área do rótulo tem o link e o texto antigos substituídos no mesmo `Edit::Batch` — achados **pela marca, nunca pela geometria**: o rótulo é o item adicionado (`/PCMark /Added` com `/PCAdded` cujo texto é o `link_text`, `edit/src/added.rs:1-9`) e o link é o que tem a URI `https://lichess.org/analysis/…`; texto do livro ou um rótulo que o usuário editou nunca é apagado (ou pulado com o motivo "já tem link para outra posição", conforme a opção).
- *Verificação:* rodar duas vezes com uma correção no meio deixa 1 link, com a URL nova (falha antes); um diagrama impresso do lado das pretas dá a mesma URL (com `?color=black`) no editor, no tabuleiro e no link gravado no PDF.
- *Esforço:* M.
- *Reversão:* os links já gravados ficam no PDF (um passo de desfazer cada); o build revertido volta a empilhar rótulos (A4-09); revert.

**A9-G10 — Correções com desfazer e rascunhos que não se perdem.** Achados A6-04, A5-06, A5-09.
- *Tarefas:* Confirmar e Esquecer viram passos do histórico de xadrez (escopo D12 do plano A8) com aviso "Corrigido · Desfazer" pelo componente `notice(text, action)` de §2.8.1, criado aqui (hoje `notify` só aceita texto, `lib.rs:2058`; o I17 o reaproveita); desfazer um Esquecer grava uma **nova revisão** do mesmo `uid` com `deleted = false` (a regra de mescla das correções nunca desfaz uma remoção, `corrections.rs:366-367`, e passa a rodar de fato com o `K::merge` do G3); Esquecer com rascunho pergunta; a aba Diagrama decide o escopo do Ctrl+Z num lugar (sem passo no editor → desfazer **da correção** da aba, nunca da partida do tabuleiro); `answer_chess_draft` mantém a pergunta aberta com o motivo quando o Confirmar falha e avisa a cada tentativa; `drafts.retain` de documentos fechados; "Redesenhar no PDF" **grava a correção antes do desenho** (hoje `chess_redraw.rs:63-79` desenha primeiro e só avisa se a gravação falha — o caso fica real depois do G2, quando um build antigo abre as correções v2 só para leitura). ("Continuar editando" de outro documento vai para I4.)
- *Verificação:* confirmar → `edit.undo` habilitado → desfazer → Lido; esquecer → desfazer → a correção volta e sobrevive a uma escrita de outra instância pelo `update` sob a trava; e4 no tabuleiro + Ctrl+Z na aba sem histórico → a partida mantém e4; store só-leitura + Sair → a pergunta continua aberta com aviso.
- *Esforço:* M. Depende de G3 (escrita do store).

#### Onda F — PGN fiel

**A9-F1 — Bytes → texto num lugar.** Achados A1-01, A8-01, A1-10 (+ a parte de hash do G4). §2.5.
- *Tarefas:* `pdfcraft_chess::pgn::decode` por linha; os quatro leitores passam a usá-lo e dizem a codificação lida quando não é UTF-8; `split_pgn`/`lex` ignoram U+FEFF inicial; o hash do PGN do livro aceita o legado e o store guarda `bytes_hash`; opção `bom` em `chess_export_pgn` e no Salvar PGN… (D18); o **kit da sessão da D18**: os arquivos de teste (UTF-8 com e sem BOM, Windows-1252, `$37`/`$23`, marcas de diagrama `$201`/`$220`, um exercício com `%tqu`), o roteiro de 15 minutos e a planilha de respostas em `docs/chess/analise-9/d18/`.
- *Verificação:* `split_pgn("\u{feff}[FEN …]\n\n1. e4 *")` = 1 pedaço com o FEN; `decode(b"[White \"R\xe9ti\"]")` contém "Réti" com `Windows1252`; um arquivo com uma partida UTF-8 e uma Latin-1 dá "Réti" nas duas (`Mixed`); `chess_games` com `bom.pgn` → `count: 2` (falha antes); um store com um PGN Latin-1 guardado pelo caminho antigo + o mesmo arquivo lido pelo F1 → as decisões ficam.
- *Esforço:* S–M.

**A9-F2 — Um scanner e o leitor em três passos.** Achados A1-03, A1-05/A7-01, A1-06 (leitura), A1-09, A1-15, A1-16, A1-17, A1-21. §2.5.
- *Contexto:* `chess/src/game.rs:1100-1610`, `game/split.rs`, `chessvision/src/doc/pgn.rs:284-667`.
- *Tarefas:* `game/pgn_scan.rs` + `game/pgn_read.rs` (`scan` → `parse` em `Vec<FragmentItem>` → `build` = núcleo do graft, compartilhado com a captura); `Nag::Written`/`Nag::WhiteRelative`; `split_pgn` sobre o scanner; regras de §2.5; `PgnReport.stop_reason`; as ferramentas devolvem `pgn_stop_reason`; a mensagem da UI usa o motivo.
- *Verificação:* golden com as 18 sondas de A1 e as de A7-01; `CORPUS` existente igual; python-chess como oráculo; a variante `2.Nf3 (2.Nf3 Nc6 3.Bc4)` mantém 3.Bb5 como principal; proptest de `split_pgn`; 100.000 `(` numa thread de 256 KiB → `Ok` com aviso `TooDeep`. Mutação por amostra (Q4) ≥ 90 % em `pgn_read.rs`.
- *Saída:* CC de `from_pgn_report`/`lex` substituídas por funções < 20. *Esforço:* M–L, em dois PRs: **F2a** (scanner + `split_pgn`) e **F2b** (leitor em três passos).

**A9-F3 — Nada cortado em silêncio.** Achados A1-02, A1-08, A1-12, R2-02, A1-20, A2-18 (limites), A4-05 (raiz), A4-22.
- *Tarefas:* avisos `CommentCut`, `TagCut`, `TagsDropped`, `NagDropped`, `NagReplaced`, `ResultIgnored`, `FenRepaired`; **um teto só, de leitura e de escrita**, em bytes, nos dois crates (subir só o de leitura faria `set_comment` cortar em 4.096 ao editar um caractere de um comentário lido com 7.999); `set_comment`/`set_comment_before` devolvem `Cut { kept }` quando cortam, e todo chamador avisa ou recusa; FEN tolerante; resto não jogado como comentário; uma função `already_says(existing, text)` (igual, ou terminando em " " + texto) usada pelo graft **e** pela captura em modo comentário (`capture.rs:627-640`), com `already_there` no relatório do graft e `PreviewReason::AlreadyThere` na captura ("O comentário já diz isso"); `rename` devolve etiquetas cortadas/recusadas.
- *Verificação:* sonda p17 (comentário de 7.999 bytes) lida inteira e editada sem corte; um aviso por etiqueta/NAG descartado; as 6 FEN de A1-08 abrem com aviso; o roteiro `s13.json` regravado contém `Bxh7` e o comentário; a captura de "strong" num comentário "White's centre is strong and solid" acrescenta o texto ou diz que já está (hoje responde `applied: true` sem mudar nada).
- *Esforço:* M.

**A9-F4 — Um escritor só.** Achados A1-11, A2-09, A1-06/A2-02 (escrita), A2-18, R6-08. §2.5. Também: `PlyCount` recalculado (ou tirado) quando os lances mudam; `ECO` lido mantido, mas tirado da escrita quando a linha principal muda antes do 10º lance; verificação: editar o 3º lance de uma partida com `[PlyCount "80"]` → o PGN gravado tem o `PlyCount` certo (falha antes: 80).
- *Tarefas:* `pdfcraft_chess::pgn::write` com quebra em 79 e `comment_before` depois das variantes do pai; `Game::pgn`, `doc::pgn::write_with` (as partidas do documento via `to_game`, com `dropped` como aviso) e `annotate` passam a usá-lo; `positions` respeitado com referência; `doc::pgn::parse` apagado, testes do documento relidos por `Game::from_pgn`. Decisão D19 (cópia para a área de transferência).
- *Verificação:* `chess_export_pgn` com e sem referência → nenhuma linha > 79 salvo token maior, mesmas convenções; `write(read(s)) == s` para `1. e4 {A} (1. d4 d5) {B} 1... e5 *` nos dois caminhos; propriedade-guia de §2.5 sobre o corpus.
- *Esforço:* M.
- *Reversão:* os PGN já gravados são PGN válido; revert.

**A9-F5 — Semântica do jogo.** Achados A1-04, A1-07, A1-13, A1-18, A1-19, A1-22, A1-23, A1-25, R6-04, R6-05.
- *Tarefas:* `result()` devolve a etiqueta lida, `outcome()` diz o que o tabuleiro diz, e a divergência vira `PgnWarning::ResultMismatch` (§2.5; nunca reescrever); `Variant` (§2.5); `Nag::code_after(white_moved)` única para os três lugares (corrigir o teste `nag.rs:475`); constante única de letras de roque (`O 0 o О о`); `San::read` recusa promoção de peça e fora da última fileira; `promote() -> Result`; `decode.rs` usa `rules::promotions_needed`/`missing_pawns` + proptest "decode satisfeito ⇒ `fatal_violations` vazio"; `next_variation` com ordem de leitura em cache por revisão; `Mate(0)` = lado a jogar perdeu.
- *Verificação:* um teste por item, cada um visto falhando (ex.: `[Result "1-0"] … 99 80` mantém `1-0`; `[Result "1/2-1/2"]` + `2... Qh4#` mantém `1/2-1/2` com `ResultMismatch`; um PGN Chess960 com FEN Shredder lido (ou recusado com o motivo) e um Crazyhouse recusado inteiro, nunca cortado; `nags_from_suffix("⨀", true) == [23]`; `San::read("О-О", Ru)` é roque; `San::read("Nf4=Q")` é erro).
- *Esforço:* M (vários S; pode ir em dois PRs).

**A9-F7 — Comandos de comentário e dialetos.** Achados R6-01, R6-06. §2.5.
- *Tarefas:* `split_commands` no `pdfcraft_chess`; a lista, a trilha, o comentário grande e as exportações de documento mostram só a prosa; selos de tempo e de avaliação opcionais; o escritor mantém os comandos; `PgnDialect::Lichess` no escritor do F4; `$201` como marca de diagrama.
- *Verificação:* a partida do lichess com `[%eval]`/`[%clk]` → prosa vazia nos lances e nenhum marcador de comentário (falha antes); ida e volta mantém os comandos; com o dialeto Lichess, `$37` sai `$36` e um `$146` desconhecido sai como comentário; `$201` é diagrama.
- *Esforço:* S–M. Depende de F4 (o dialeto).

**A9-F6 — Operações tipadas do `chess_pgn_edit`.** Achado A7-12.
- *Tarefas:* `play` sem `clone` da partida por operação (aplicar e, na falha, cortar os nós acrescentados; hoje 1,15 ms por `play` numa partida de 10 mil lances, R8-08) e `MAX_OPS` ponderado pelo tamanho da partida; `enum PgnOp` com `serde(tag = "op")` e tipos validados; esquema JSON por `schemars` (declarado) ou à mão com teste de paridade enum × esquema.
- *Verificação:* os testes das 17 operações passam iguais; `op` desconhecido dá `unknown op "<x>"` (igualdade de texto); campo a mais continua aceito.
- *Esforço:* M.

#### Onda L — leitura do livro

**A9-L0 — Leitura e walk separados.** Achado R3-01. §2.6. Vem antes de L1, L2, L3 e L8. Dois PRs: **A9-L0a** (o walk puro, `MoveInline` só com o lido, `READER_VERSION`) e **A9-L0b** (`link` = walk com overlay, decisões por `san_read`, orçamento por fronteira de página).
- *Contexto:* `chessvision/src/doc.rs:600-660` (`add_read_page`), `doc/build.rs:1040-1060`, `doc/games.rs`, `doc/reference.rs:460-560`, consumidores de `MoveInline` (aba Texto, `span::join`, exportações).
- *Tarefas:* (0) `doc::READER_VERSION = 1` (só a constante; gravá-la no `ReferenceFile` é do L1b, sobre a v2 do G3a); `MoveInline` só com o que foi lido (`san_read` no lugar do `san` reescrito; bandeiras do leitor); `walk(&[PageReading], rules, overlay) -> Walk` puro com o resultado por inline e por diagrama; `add_read_page` deixa de zerar campos; `link` passa a ser "walk com overlay + alinhar" e deixa de desfazer a si mesmo; decisões casadas por `san_read` + ordinal (com o recasamento de §2.6 para as já gravadas).
- *Orçamento:* o walk do livro inteiro custa até ≈ 5,2 s com a varredura em cache (748 pp., 7 ms por página, extraindo o texto); o overlay de uma correção re-percorre só a partir da partida afetada — estado do walker guardado na fronteira de cada página, `PageReading` em cache —, nunca o livro inteiro por correção, nem no thread da UI.
- *Verificação:* o teste de R3-01 como regressão: para as posições F2 e F3, `walk(ler(F1))` com overlay F2 = `walk(ler(F2))` (hoje: bandeiras `cxd4→exd4` e `cxd4→cxd4` a mais; em F3 a SAN fica `exd4`); propriedade "walk duas vezes = uma"; Q2 sem perdas (os números do walk não mudam fora dos casos de overlay).
- *Esforço:* M–L.
- *Reversão:* o L0a só cria a constante; depois do L0b, um build revertido trata as decisões casadas por `san_read` como `decisions_unmatched`, nunca as aplica por endereço (regra do `reader_version` maior, §2.10); revert.

**A9-L1 — Papéis dos blocos.** Achados A2-04, A2-06. §2.6. Depende de A9-L0b. Dois PRs: **L1a** (o tipo `role` e o build) e **L1b** (as exportações pulam o que não é conteúdo; recasamento das decisões e `reader_version` gravado no `ReferenceFile`).
- *Contexto:* `chessvision/src/doc.rs:227-231`, `doc/build.rs:700-1000`, `html.rs`, `markdown.rs`, `engine/src/chess/office.rs`, `reference/annotate.rs`.
- *Tarefas:* `role` nas variantes `Heading`/`Paragraph` (§2.6; ~56 construções e padrões, 35 fora de testes); `folio_blocks` grava o papel da página; o cabeçalho corrente sai do `Walk` (A9-L0); linhas dentro da caixa de um diagrama viram `DiagramText`; walker e exportações pulam o que não é conteúdo; folio como rótulo de página no EPUB sem `/PageLabels`; recasamento das decisões gravadas (§2.6 "Endereços estáveis").
- *Verificação:* duas páginas com "Issue 47" na mesma altura e folio "15" → um só título, nenhum `<p>15</p>`, `toc().len() == 1`, PGN anotado sem `{15}`; o probe2 de A2 refeito com uma fonte de diagrama de família desconhecida (a Merida com a tabela `name` trocada para "BookDiagram", que nem o nome do PDF nem o do programa pegam; o probe2 original passa no `main` desde o #254) → nenhum título-lixo e os lances depois do diagrama válidos; uma decisão gravada sobre a leitura antiga de uma página com folio continua aplicada; Q2 sem perdas.
- *Esforço:* M–L.
- *Reversão:* o `reader_version` gravado maior que o do build revertido deixa o arquivo só para leitura (§2.10); revert.

**A9-L2 — O texto do diagrama não se perde.** Achados A2-01, A2-02 (walker).
- *Tarefas:* `held_text` com teto; reprocessar em `resolve_held`; comentário inicial quando a partida começa no diagrama; texto depois das variantes de um lance vai para `pending` (vira `comment_before` do próximo lance principal).
- *Verificação:* o caso E1 de A2 (`assert!(pgn.contains("key idea"))`); `(3...Nf6 4.O-O [diagrama] Berlin wall.) 4.Ba4` sem `ImpliedClose`; `held_loss.py` sobre dv100/fi30/flores/ce60 → 0 trechos perdidos.
- *Esforço:* S–M.

**A9-L3 — Parágrafos e hifens.** Achados A2-05, A2-15.
- *Tarefas:* continuidade pela geometria; dicionário de hifens do próprio documento.
- *Verificação:* `splits.py` no Harikrishna 60 pp.: 105 → ≤ 10 parágrafos partidos no meio da frase; "well-/known" mantém o hífen e "posi-/tion" junta; uma decisão gravada sobre uma página com hífen e parágrafo partido continua aplicada (§2.6).
- *Esforço:* S–M.

**A9-L4 — Lances de prosa medidos pela verdade.** Achado A2-03. Depende de Q2.
- *Contexto:* as classes medidas pela R6 sobre `hk60.json` (56 lances sem número logo depois de prosa): peça depois de determinante 24 (1 confirmado, 7 recuperados, 16 ausentes/em conflito); casa nua depois de preposição ("on h5", "to d3") 15 (0, 3, 12); depois de conectivo 4 (0, 0, 4); outras 13 (1, 5, 7). Total: 2 confirmados, 15 recuperados, 39 ausentes ou em conflito.
- *Tarefas:* o `walk_bench --reference` reporta essa tabela por classe; cada regra (casa depois de preposição; peça depois de determinante) entra sozinha, e só se não perder confirmados nem recuperados; as ideias `…d5` continuam no texto (32 nas 60 pp.; o teste guarda isso). O PGN do livro não é a verdade absoluta (edições digitais reordenam e acrescentam linhas): o bench lista os conflitos por classe com uma amostra auditada à mão, e não otimiza só "confirmados".
- *Verificação:* "…does not come to d3. 20.Rxd3" → d3 Unchecked, 20.Rxd3 válido; Harikrishna 60 pp.: lances sem número depois de prosa com ligação `absent`/`conflict` 39 → 0, e os 2 confirmados e os 15 recuperados **continuam lances**.
- *Esforço:* S.

**A9-L5 — Toda troca registrada e a notação conferida.** Achados A2-10, R6-02.
- *Tarefas:* palavra original até `ChessRun::Move` (`repaired_from`) e `ReviewFlag::TextRepaired`; o PGN do livro (passo R2 do `plano-pgn-do-livro.md`) e a revisão passam a ver a troca; `San::to_move_checked` devolve o lance e `NotationMismatch { capture, check, mate }` (marca de captura que não confere; `+`/`#` **a mais** — muitos livros continentais não imprimem o xeque, então a falta não conta); o walk grava `ReviewFlag::NotationMismatch` e trata o lance como evidência fraca nas regras de casamento do walker e do alinhamento.
- *Verificação:* linha "6.Rel" → bandeira presente com `from: "Rel"`; "8.Nxd8" para casa vazia → `NotationMismatch` (hoje `8. Nd8+` sem bandeira); Q2 sem perda de confirmados.
- *Esforço:* S.

**A9-L6 — Idioma do texto e fatos do diagrama no dado.** Achados A2-08, A1-14, R3-02 (orientação), R5-05, R6-03. Depende de A9-L0b. Três PRs: **L6a** (`text_lang`) · **L6b** (`DiagramReading`) · **L6c** (`Stipulation` + `declare_side`).
- *Tarefas:* `ChessPage.text_lang` e `lang` como `Lang` validado; exportações usam `text_lang` para `lang`, `dc:language`, `w:lang` e `describe`; `DiagramReading { fen: Fen, side: SideToMove, orientation, … }` com as bandeiras da leitura derivadas (§2.6); `describe` recebe a leitura (o lado suposto sai certo nas três exportações sem que cada uma o trate); `number`/`index` do `DiagramBlock` viram um.
- *Tarefas (enunciado, R6-03):* parser de `Stipulation` no `pdfcraft_chess` e o seu uso no `declare_side` com `SideSource::Text`/`TextPageScope`; o N4 usa o mesmo enunciado.
- *Verificação:* página sintética com "Black to move" sob o diagrama → `b`, sem `SideAssumed`; cabeçalho "Pretas jogam" vale para os diagramas da página; um lado impresso que contradiz a legalidade → `SideConflict`; probe3 de A2 (prosa pt-BR, figurinas, `/Lang (pt-BR)`) → `lang: "pt"` e "Diagrama 1, pág. 1"; diagrama com lado suposto sem "brancas jogam"; página sintética com "Position after: 5.dxc5" e o diagrama → "pretas jogam" e link com `%20b%20`; Harikrishna pp. 12–15 nos três formatos: 6 de 6 certos (hoje 2 de 6); o teste de R3-02 invertido (um diagrama do lado das pretas confirmado pelo PGN mantém a orientação).
- *Esforço:* M.

**A9-L7 — Tetos, rótulos e notação descritiva.** Achados A2-07, A2-12, A1-24, R6-07. Detectar a notação descritiva ("P-K4", "N-KB3", "PxP", "P4R", "C3BR") e dizer na nota da página "notação descritiva: não suportada" em vez de 0 partidas mudas, com entrada Missing no `parity/`; converter é a D27.
- *Tarefas:* `MAX_GAMES` 20.000 + `ReviewFlag::TooManyGames` + nota no resumo; `is_label` aceita rótulos maiúsculos/numerados como separador; variante sem lance volta ao comentário com o colchete original; 3+ `=`/`-`/`*` sozinhos são prosa; `looks_like_chess_font` exige `chess`/`dg`/`fig` junto do nome de cidade.
- *Verificação:* E4 de A2 com a bandeira; testes de unidade para os rótulos; `!looks_like_chess_font("ZurichBT-Roman")`.
- *Esforço:* S.

**A9-L8 — Re-percorrer depois de ligar ao PGN do livro.** Achado A2-11.
- *Tarefas:* em `link`, se algum diagrama confirmado mudou o FEN, chamar o walk do A9-L0 com o overlay e alinhar de novo uma vez (sem código de re-percurso próprio). Depende de A9-L0b.
- *Verificação:* E7 de A2: válidos ≥ antes (+18 na amostra); os 5 lances que pioram na amostra (E7) listados um a um, cada um corrigido ou explicado no PR; idempotência de `link` mantida.
- *Esforço:* S.

**A9-L9 — Regras do walker como funções puras.** Achado A7-10. Depende de Q2 e L4 (os dois reescrevem o `Walker::play`; em sequência, nunca em paralelo). Dois PRs (regras de abertura e de variante; o resto).
- *Tarefas:* `rule_*` + `Placement`; `play` = a primeira que decide + `apply`; testes por regra (3–5 textos cada); `walk_bench --rules=-x` mede o peso de cada uma.
- *Verificação:* `walk_bench --baseline` com 0 perdas nos 10 documentos; CC de `play` < 20.
- *Esforço:* L (refatoração pura; sem mudança de comportamento).

#### Onda V — visão

**A9-V1 — SVG de peça sem recursão hostil.** Achados A3-01, R2-01 (C).
- *Contexto:* `chessvision/src/pieces/svg.rs:51-115`; chamadores `pieces/images.rs:118`, `pieces/import.rs:260,275`; roteiros de R2 (`mk_use_chain.py`, `mk_chains.py`).
- *Tarefas:* (1) antes de `from_xmltree`, um percurso **iterativo** do `roxmltree` mede a profundidade do aninhamento **e** a da cadeia de referências IRI (`href`/`xlink:href` e `url(#id)` em `clip-path`, `mask`, `filter`, `fill`, `stroke`, `marker-*`), com conjunto de vistos e teto de 32; recusa acima de qualquer um com "SVG aninhado ou encadeado demais" (peças reais: < 6 níveis, cadeias de 1–2); (2) no desktop, `from_xmltree` + `collect` rodam num thread com pilha própria e fixa (`stack_size(16 MiB)`) como salvaguarda para a forma de recursão que o percurso não previu (no wasm não há thread: só (1) protege, e isso fica escrito); (3) `collect` iterativo (pilha explícita de `&Group`).
- *Verificação:* quatro casos numa thread de 1 MiB de pilha, cada um abortando antes e devolvendo `Err` depois: 600 `<g>` aninhados, 300 `<use>` encadeados, 3.000 `clip-path`, 3.000 `mask`; `chess_pieces_import` recusa o `wK.svg` e diz por quê; os 12 conjuntos que acompanham o app carregam iguais.
- *Saída:* parity das peças importadas diz que SVG aninhado ou encadeado demais é recusado. *Reversão:* revert. *Esforço:* S–M.

**A9-V2 — A tinta da página calculada uma vez, com piso de área.** Achados A3-02, A3-09.
- *Contexto:* `glyphs.rs:60-70,240-340`, `glyphs/seg.rs:280-310`, `glyphs/weight.rs:120-240`, `engine/src/chess/text_ocr.rs:180-260`.
- *Tarefas:* `PageInk { crop, bin, components, scale }` calculada uma vez por página e passada a `text_scale`, `frames`, `read`, `bolder_lines`, `bold_words`; componentes abaixo de um piso de área (`MIN_AREA_GLYPH × escala²`, mínimo 3 px) descartados sem contar; um teto só para "não é texto"; `frames`, `bolder_lines` e `bold_words` são dicas de leiaute: falha vira vazio com nota, nunca derruba a página.
- *Verificação:* a página sintética c06 de A3 (40 linhas + meio-tom de pontos de 1 px) lê as 40 linhas (falha antes); também com um meio-tom de 85–100 lpi a 300 dpi (pontos de 2–3 px, que o piso de área não tira — com escala 28 o piso é 3,9 px²) e com uma foto escaneada real; para o que ainda passar do teto, a página é lida por faixas e só a faixa estourada é pulada, com nota; `read_page` idêntico no corpus sintético do engine; tempo fora do `read` cai de 115–170 ms para ≤ 50 ms (perfil de teste).
- *Esforço:* M.

**A9-V3 — O leitor de glifos em lotes.** Achado A3-03.
- *Tarefas:* juntar os pares candidatos a fusão da página e lê-los num lote (≤ 1.024 células); aplicar as fusões em ordem e reler em lote só as uniões novas que encostam no vizinho; linhas também em lotes de página.
- *Verificação:* classificador falso determinístico: texto lido idêntico ao de hoje e ≤ 10 chamadas na página itálica (hoje 1.960); com o modelo real, um lote de outro tamanho pode mudar `p` na última casa e virar o limiar do árbitro, então "idêntico" vale só para o falso, e o real é medido por `ocr_bench --gate` sem perda (CER e lances).
- *Esforço:* M.

**A9-V4 — Cifra do livro por página, guardada, sem autoevidência.** Achados A3-04, R3-10.
- *Tarefas:* evidência guardada por página (`BTreeMap<u32, Vec<…>>`); `record(proof, page)` substitui a contribuição da página; `proven()` soma as páginas; a leitura da página p usa `proven_without(p)`, para que reler p dê o mesmo resultado (hoje `read_page` repara com a evidência que a própria página deu, `text_ocr.rs:253-255`); a cifra vira `StoreKind` "cipher" (G3) e passa a ser guardada; no `cipher_out` da CLI, a regra do legado de §2.10.
- *Verificação:* gravar a mesma página duas vezes = uma vez (falha antes: dá `proven = {"W": Queen}`); páginas 12 e 13 somam; ler p, ler p de novo → o mesmo texto; um `cipher_out` novo lido pelo código do build anterior (fixture) não vira tabela vazia.
- *Esforço:* S–M, em dois PRs: **V4a** (onda 4) evidência por página e `proven_without(p)`, sem esperar nada; **V4b** (com o G3c) a cifra guardada como `StoreKind` "cipher", na sessão do motor (§2.7), nunca no `DocChess`.
- *Reversão:* a cifra guardada tem versão própria: um build revertido não a lê e volta a reler a cifra por página (perde só o atalho); revert.

**A9-V5 — Fontes embutidas só com licença verificada.** Achados A3-05, A3-06.
- *Tarefas:* exportações aceitam só TTF/OTF (WOFF recusado com "converta para TTF"), ou WOFF 1.0 com a `OS/2` inflada para ler `fsType` e WOFF2 recusado; `embedding_forbidden(fs) = fs & 0xF == 2 || fs & 0x0200 != 0`.
- *Verificação:* `from_bytes(b"wOFF", "x")` → `Err(NotAFont)`; `fsType` 0x0200 → `Err(Restricted)` (passam hoje).
- *Saída:* nota do parity de EPUB/HTML. *Esforço:* S.

**A9-V6 — Peças importadas sem travar a interface e sem perder o conjunto antigo.** Achados A3-07, R7-02.
- *Tarefas:* troca atômica do conjunto: `write_set` e `import_fonts` gravam numa pasta irmã `.<nome>.import-<pid>`, validam com `from_image_dir`, e trocam por `rename` (a antiga vira `.<nome>.old-<ts>`, a nova vira `<nome>`, a antiga é apagada; se algo falhar no meio, a troca é desfeita); fontes passam pelo gravador atômico; reduzir por média de área toda imagem maior que `TARGET_GRID` antes de traçar; importar e montar a prévia num `Job` (I2); conjunto traçado guardado por (caminho, tamanho, mtime) para validar, pré-visualizar e escolher com um só traçado.
- *Verificação:* um arquivo do conjunto antigo aberto sem `FILE_SHARE_DELETE` durante `replace: true` → erro, e o conjunto antigo intacto com 12 arquivos e os bytes de antes (hoje sobram 6 de 12); `trace` de um disco de 2048 px dá o mesmo contorno (Hausdorff < 0,5 % do lado) que o de 512 px em menos de 1/8 do tempo; kittest: Importar não bloqueia o quadro.
- *Esforço:* M, em dois PRs: **V6a** (troca atômica do conjunto; S; onda 1, faixa C) e **V6b** (importar e pré-visualizar num `Job`; depois do I2).
- *Reversão:* os conjuntos já trocados ficam; pastas `.<nome>.import-*`/`.<nome>.old-*` órfãs são listadas pelo `chess_status`; revert.

**A9-V7 — Detectores lineares.** Achado A3-08.
- *Tarefas:* `detect/font.rs::boards`: as linhas já são ordenadas por `cy` (`:216-220`); falta a busca binária da janela `[y + 0,75·casa, y + 1,25·casa]` e calcular `aligned` só depois do teste de passo; `detect/vector.rs::lattices` com uma grade espacial (`HashMap<(i32, i32), Vec<usize>>`, célula de `8·lado`, 9 vizinhas); cancelamento checado dentro da página.
- *Verificação:* proptest de equivalência com o algoritmo atual em entradas aleatórias; 200 mil glifos e 50 mil caixas abaixo de 1 s **em release** (hoje 2,85 s e 1,58 s em release).
- *Esforço:* S + S.

**A9-V8 — Modelo conferido antes e depois de carregar** (onda 0, como o V1: mesma classe, aborto). Achados R7-01 (C), A3-10.
- *Tarefas:* (0) **antes** de `Model::load`, ler o grafo ONNX (protobuf, um leitor mínimo de `ModelProto.graph.node[].op_type` e dos `dims` dos inicializadores, sem o `rten`) e recusar com `ModelError::Unsupported { op | init }`: op fora da lista {Conv, Relu, MaxPool, Gemm, MatMul, Add, Flatten, Reshape, Constant, BatchNormalization, GlobalAveragePool, Dropout, Identity, Softmax, Clip, Sigmoid} (os dois modelos de hoje usam {Constant, Conv, Gemm, MaxPool, Relu, Reshape, Flatten}); subgrafos (`If`/`Loop`/`Scan`); inicializador ou `Constant` acima de 2^24 elementos; (1) só então carregar e, se existir `<nome>.parity.json` ao lado, rodar as 3 entradas e recusar com `ModelError::Parity { worst }` acima de 1e-3 (confere também a forma da saída) — a paridade nunca roda num grafo não conferido; opcional: `sha256` no JSON pelo script de exportação.
- *Verificação:* o modelo `bomb` da R7 dá `ModelState::Invalid("…ConstantOfShape…")` numa thread (hoje aborta o binário); um modelo com um bit trocado num deslocamento conhecido (um dos 8 casos de A3) vira `ModelState::Invalid("…parity…")`; os dois modelos reais carregam iguais.
- *Esforço:* S–M.

**A9-V10 — A varredura usa os núcleos.** Achado R8-02. Dois PRs: **V10a** (onda 4: `run_parallel` no motor e na CLI, sem UI) e **V10b** (onda 5, depois do I2: a UI usa o paralelo pelo `Job`).
- *Tarefas:* `ChessScanJob::run_parallel(models, workers, progress)` com `workers = min(available_parallelism − 1, 4)`; cada worker com o seu `PageRenderer` e o `catch_unwind` por página que já existe; resultados em ordem de página; cancelamento checado por página; o pool de inferência do `rten` limitado para não sobrescrever os núcleos; memória com teto de `workers × pico de uma página` (medido: 120 MB detectando e 285–445 MB lendo, pico do processo inteiro); no wasm, sequencial e cooperativo (I2).
- *Verificação:* propriedade "paralelo = sequencial" (o mesmo `ChessScan`, campo a campo) em PDFs sintéticos; `perf_` com o Harikrishna 100 pp. local ≤ 0,4 × o tempo sequencial; meta do livro inteiro ≤ 1,5 min (§5).
- *Esforço:* M.

**A9-V9 — Um Douglas–Peucker.** Achado A3-11.
- *Tarefas:* o `simplify_open` iterativo de `detect/contour.rs` vai para `imageops` (genérico sobre `(f32, f32)`) e substitui o recursivo de `pieces/images.rs`.
- *Verificação:* mesma saída nos testes atuais de `trace`.
- *Esforço:* S.

#### Onda X — exportações

**A9-X1 — Formatos que não quebram.** Achados A2-13, A2-14, A2-17, A4-18, R8-09. Dois PRs: **X1a** (escapes e ZIP; S; onda 3) e **X1b** (DOCX do tamanho certo; M; depois da sessão da D18).
- *Tarefas:* Markdown escapa `&` e parágrafos feitos só de `-`/`=`/`_`/`*`; caminhos de imagem com percent-encode por segmento (RFC 3986); escapes XHTML/SVG filtram U+FFFE, U+FFFF e U+FDD0–FDEF; `Zip::add -> Result` recusa > `u32::MAX`, nome > `u16::MAX` e a entrada 65.536, propagando o erro do deflate; DOCX do tamanho certo (R8-09: hoje 118,5 MB para o Harikrishna): PNG de reserva indexado (poucas cores) no tamanho que o Word mostra, peças do SVG uma vez por documento se o `svgBlip` do Word aceitar `<use>` (conferir na sessão da D18), e a resposta da ferramenta dizendo o tamanho.
- *Verificação:* DOCX do Harikrishna inteiro ≤ 25 MB (`perf_`); `out = "Livro #2 100%.md"` → links resolvem; parágrafo "---" e "&copy;" literais; `escape("a\u{FFFE}b")` sem o não-caractere; `Zip` com `count = u16::MAX` recusa a próxima.
- *Esforço:* S.

**A9-X3 — O que um leitor cego ouve no livro exportado** (onda 3, S–M). Achados R5-06, A1-14 (com L6).
- *Tarefas:* sinais no HTML/EPUB como `<span class="nag" role="img" aria-label="Brancas estão claramente melhor">±</span>` (hoje só `title`, que os leitores não leem: "±" sai "mais ou menos") e, no DOCX, o nome do sinal entre parênteses quando não é `!`/`?`; `diagrams: fen` com a descrição além do `<code>`; marca de revisão no HTML com texto oculto "a revisar: <motivo>" e borda ≥ 3:1 (hoje `#fff3c4`, 1,11:1, só cor); figurinas Unicode sem fonte de letras com `role="img" aria-label` no idioma do livro (hoje `♞` sólido para os dois lados: "black knight" num lance branco); metadados EPUB **derivados do conteúdo** (o `accessibilitySummary` diz quantos diagramas têm o lado vindo do livro e se os números de página são os impressos — hoje é uma constante que afirma as duas coisas sempre, `epub.rs:61-69`), no idioma do livro ou com `xml:lang="en"`; `dc:creator` omitido quando desconhecido; `<title>` de cada página "Livro — pág. 13"; sumário aninhado por nível; Ace by DAISY (MIT) e epubcheck (BSD-3) como oráculos externos opcionais em Q2, com pulo sem eles — nada de `dcterms:conformsTo` enquanto o Ace não passar.
- *Verificação:* um teste por item, cada um visto falhando, sobre o probe sintético de A2.
- *Esforço:* S–M.

**A9-X2 — Diálogo de exportação.** Achado R4-04. Com o aceite de acessibilidade próprio (`Dialog`, campos com `labelled_by`) e a opção "Descrição dos diagramas: letras (Rg1) / palavras (rei g1)" para síntese de voz. §2.8.1 "Exportação com diálogo".
- *Tarefas:* um diálogo no lugar dos submenus de rádio (`chess_export_ui.rs:53,61-89`) com formato, páginas, origem das partidas, diagramas como partidas, idioma do livro e o resumo do que sai; aviso de pendências dentro dele; a frase sobre as partidas do tabuleiro até o N2; a escolha do PGN lembrada (pendência do P9c do plano A8).
- *Verificação:* kittest: o resumo mostra as contagens e o idioma; mudar o idioma muda o `lang` do EPUB exportado; a jornada "livro → EPUB salvo" cai para ≤ 5 gestos.
- *Esforço:* M.

#### Onda I — interface

**A9-I15 — Rápidos e visíveis** (onda 1; arquivos de UI pouco disputados). Achados R4-01, A6-02/A6-03 (via I7), A6-16, A6-18, R4-05, R4-06, R4-08, R4-11.
- *Tarefas:* (1) **modo Ler corrige diagramas** (R4-01): no modo Ler, `edit_chess_diagram` mostra o editor dentro do painel, pelo caminho do editor clássico (`chess_ui.rs:738-744`), em vez de empilhar um `pending_panel_reveals` que só o `docking::show` consome (`docking.rs:451-463`); (2) o I7 inteiro (oferta do PGN); (3) os textos de §2.8.2 que não dependem de tipo novo (chips, "Confirmar"/"Confirmado", "Alinhar o livro ao PGN", "Voltar para a janela principal", "Inserir em", "Letras das peças", "Exportar livro…"/"Salvar todas como PGN…"); (4) menus que fecham por AccessKit (estender `widgets::menu_item`; os ⋯ da lista e do rodapé da aba Diagrama); (5) alvos ≥ 24 pt (Parar, Dispensar); (6) "Brancas jogam" em `text`; (7) acessibilidade rápida, nada disso depende do I0: papel `Dialog` em `show_modal`, papel `Tab` em `widgets::dock_tabs`, o toast com papel `Status`/`Polite` (ponte até o I17), o nome da grade do editor sem a FEN ("Tabuleiro do diagrama, pág. 13, diagrama 1") e `set_bounds` nas 64 células, `RadioGroup` "Quem joga", e o nome do tabuleiro de análise por `describe::pieces` ("Tabuleiro, brancas jogam, lance 7") no lugar da FEN.
- *Verificação:* kittest no modo Ler: selecionar um diagrama mostra o editor (falha antes) e sair do modo Ler não revela de repente uma aba aberta antes; `click` por AccessKit fecha os menus; todo nó clicável do painel ≥ 24 × 24; o modal tem papel `Dialog`; o toast tem `live`; toda `GridCell` tem `bounds` e o nome da grade não tem "/".
- *Esforço:* S–M (dois PRs se preciso).

**A9-I0 — Arquitetura da informação do xadrez.** Achados R4-03, R4-12, R4-13, A6-12, A6-14 (Original/Lido), A6-24, R5-04. §2.8.0. Depois do I1b (os dois mexem em `chess_ui.rs`); I0a → I0b → I0c; antes de I8, I9, I11b, I13, I14, I16, I17, N1, N3 e N7b. Absorve o antigo I12, a parte "Original / Lido" do I13, o título "Xadrez" e os avisos fora do rodapé do I14, e a oferta de recolher Ferramentas (P5 do plano A8).
- *Contexto:* `docking.rs:74-121,293-306,451-463,628`; `chess_ui.rs`; `chess_board_ui.rs:1234,1320-1324` (`MIN_NOTATION = 120`, decisão "spec §4.1" do plano de peças que este passo revê); `chess_window.rs:74,811`; `lib.rs:3444-3486` (modo Ler); capturas `14`, `24`, `46`, `54`, `r04`.
- *Tarefas:* três PRs — **I0a** coluna Xadrez com abas e mestre-detalhe, inclusive no modo Ler e com o orçamento de colunas por largura; **I0b** tabuleiro sem rolagem (regiões fixas, barra inferior Partida ▾ · Comentário · No livro · FEN/PGN ▾, "A partida começa no lance" em Partida ▾; a lista em "Colunas" virtualizada como a `flow` já é, e o campo PGN regerado só quando muda a revisão — hoje 2,1 ms por seta com 9.922 lances, R8-07); **I0c** janela destacada e pontos de quebra (colunas de 420 a partir de 1920). Começa por um kittest de leiaute que mede cada região dos esboços.
- *Verificação:* todo kittest de leiaute roda também a 680 × 352 pt (`pixels_per_point` 2): todo nó focável do painel fica com o retângulo dentro da coluna e da janela (falha antes: Exportar em x 671–697 com a coluna terminando em ~632); kittest a 1360 × 705, claro e escuro: com o editor aberto, a página tem ≥ 940 pt e o zoom "ajustar à largura" ≥ 60 % (falha antes: ≈ 390 pt e 29 %); no modo Ler o editor aparece no mestre-detalhe; no Tabuleiro, nenhum `ScrollArea` próprio e ≥ 7 linhas de lances com 3 linhas de motor; a janela de 960 × 704 mostra a lista de lances inteira; o campo do 1º lance continua alcançável por teclado e renumera a partida (teste existente); capturas a 1366 × 768 e 1920 × 1080.
- *Esforço:* L (três PRs).

**A9-I18 — Teclado do xadrez pelo foco** (onda 2, abre a faixa B; S–M). Achados R5-01, R5-11, R7-05.
- *Contexto:* `chess_tools.rs:689-733,1262-1281`; `chess_history.rs:347-361`; `canvas.rs:2423`; `craft_ui` `docking/surface.rs:774-791` (divisor por setas); `tests/chess.rs:2606-2614` (padrão `raw_input_hook` nos testes).
- *Tarefas:* (0) `persist()` parte do JSON carregado e sobrescreve só as chaves que conhece (R7-05), antes da primeira preferência nova do plano (a de atalhos de uma tecla); (1) `in_scope` = o foco do teclado está numa superfície de xadrez (painel, aba Diagrama, janela do xadrez), ou o último clique foi lá e o foco não saiu; Tab, a tecla F6 ou clique fora tiram o escopo; (2) nunca tomar uma tecla que o widget focado usa (divisor, lista, `DragValue`, combo, deslizante); (3) Preferências › Xadrez › "Atalhos de uma tecla (B, C, F, G, O, X, sinais)": Ligados/Desligados (desligados, valem os botões do trilho e a paleta); (4) **um mapa de teclas único**, gerado de `RAIL`, `NAV_KEYS`, `TYPED`, das teclas da aba Diagrama e das letras do app (V, H), que alimenta o escopo, a folha de atalhos (I16) e um teste de colisão — com ele, o `?` da folha vira a tecla F1 e o `O` do Original vira Alt+O; (5) espera dos sinais digitados configurável (0,6–2 s) ou Enter confirma (R5-11).
- *Verificação:* kittest com `raw_input_hook`, cada caso visto falhando antes: escopo ligado e divisor focado → ArrowLeft redimensiona e a partida não anda; Tab até a barra do documento e → → a página vira; preferência desligada → "B" não arma o pincel; o teste de colisão recusa `?` e `O` como estavam; um JSON de preferências com uma chave desconhecida a mantém depois de `persist()` (falha antes).
- *Esforço:* S–M.
- *Reversão:* as preferências que o build revertido não conhece se perdem na próxima gravação dele (R7-05): dito nas notas da versão (E0); revert.

**A9-I16 — Xadrez nos menus e na paleta.** Achado R4-02. §2.8.1 "Descoberta".
- *Tarefas:* comandos `chess.board.show`, `chess.analyze`, `chess.export`, `chess.shortcuts` (folha de atalhos na tecla F1 do escopo de xadrez, gerada do mapa de teclas único do I18); menu Xadrez; Ferramentas › Xadrez completo; ícone de xadrez do trilho também com fonte de xadrez ou PGN irmão.
- *Verificação:* a paleta acha "analisar", "tabuleiro" e "exportar livro" (falha antes); teste do menu; a folha de atalhos lista as mesmas teclas que o escopo executa.
- *Esforço:* S.

**A9-I17 — Avisos, anúncios, desfazer e alertas num componente** (dois PRs: os componentes + os 34 avisos de erro; os 65 de informação; o `notice` já nasce no G10). Achados R4-09, R4-10, R5-02, A6-04 (aviso), A6-26 (aviso cobrindo o rodapé). §2.8.1 "Avisos, anúncios e desfazer".
- *Tarefas:* `announce`, `notice` e `callout`; as 99 chamadas `notify*` classificadas; o toast atual migra; toda mudança de estado do usuário com Desfazer; o lance mostrado anunciado a cada navegação; fim da detecção com "Revisar agora" e anúncio; erros de modelo, de desenho e de gravação como `callout` de perigo com `Alert`.
- *Verificação:* kittest lê `role()`, `live()` e `label()` do nó depois de confirmar uma correção, de → na partida, do fim da varredura e de um erro de gravação (falha antes: não há nó vivo); dois avisos iguais seguidos geram dois anúncios; confirmar uma correção → aviso com Desfazer → o botão ou Ctrl+Z voltam a "Lido" (com G10); um modelo inválido mostra `callout` de perigo (falha antes: texto cinza pequeno); o aviso nunca cobre o rodapé da aba Diagrama (retângulos).
- *Esforço:* M.

**A9-I1 — Estado por documento.** Achados A6-01, A5-10, A5-14, A4-15, R2-05. §2.7.
- *Contexto:* `chess_ui.rs:36-141` e os ~40 usos de `self.chess.results`; `chess_text_ui.rs`, `chess_drafts.rs`, `chess_export_ui.rs`, `chess_corrections_ui.rs`, `chess_reference_ui.rs`, `chess_collection.rs:2186-2207` (`sync_chess_docs`).
- *Tarefas:* `DocChess` (só vista) e `ChessState.docs`; o estado de domínio por documento (correções, PGN do livro e decisões, cifra, origem) vai para a sessão do motor, que já é chaveada por `DocId`; as duas cópias de store da UI (`chess_corrections_ui.rs` × `chess_reference_ui.rs`) saem, e fica só o backend web que o motor recebe, já com a parte web de §2.3 (valor ilegível em `…-v1.bad`, só-leitura com aviso, mescla antes de gravar, orçamento recusado com aviso); mensagem "Detectando em outro livro…" na aba sem detecção própria; um helper `read_picked(file, cap)` com o padrão do `chess_models` (tamanho conferido antes, `Blob` lido direto, erro tratado) para os três seletores de arquivo da web (R2-05).
- *Verificação:* kittest: detectar a pág. 1 de A, a de B, voltar a A → a lista ainda tem os diagramas de A (falha antes); ler em A, ler em B → `chess_cipher(A).is_some()`; exportar A depois de detectar B mostra o aviso de pendências de A; `use_storage(Some("{trunc"))` + uma correção não sobrescreve o valor original.
- *Saída:* nenhuma vaga global de dado de documento em `ChessState`. *Esforço:* L, em dois PRs: **I1a** (`DocChess` e o domínio na sessão do motor) e **I1b** (o backend web sobre `Storage` e os seletores da web).
- *Reversão:* o valor `…-v1.bad` da web fica de lado; revert.

**A9-I2 — Um tipo de job.** Achados A5-03, A5-12, A5-15, R8-03. Dois PRs: `Job<T>` + as quatro migrações; wasm cooperativo.
- *Tarefas:* `Job<T>` (§2.7) com guarda no `Drop` e cancelamento; as quatro implementações migram; vagas separadas para a leitura de página e para os jobs longos; Parar e progresso em "Ligar o livro inteiro" e Exportar; (2º PR, wasm) `ChessScanJob::step()` e as fases do `chess_document_with` com ~50 ms por quadro, progresso e Parar — até isso, "livro inteiro" na web recusado acima de 20 páginas e o parity sem "in the background" para a web.
- *Verificação:* teste unitário do `Job` com thread real: `work` que entra em pânico → `poll` devolve `Err` com a mensagem (os testes de UI usam `run_inline = true`, onde o pânico sobe para o teste); kittest com um job longo simulado: `TextAction::Read` lê a página (falha antes); soltar o `Job` põe `cancel`.
- *Esforço:* M.

**A9-I3 — Fora do quadro.** Achados A5-02, A5-11, A5-18, R8-05, R8-06, R9-01. Dois PRs: **I3a** (onda 4: autosave incremental, mantendo o `StoredComments`) e **I3b** (onda 5: gravador por store, memo de chaves e impressões do motor, índice compacto do PGN do livro no worker). Depende de F4 (*Subtract before you add*). Também: o memo de chaves e de impressões do motor sobrevive à edição para os objetos que ela não tocou (§2.4: Título → 0 páginas rehash; uma página → 1), e o índice do PGN do livro sai do thread da UI, compacto (`BookKey` de 64/128 bits num mapa só; do PGN guarda-se só SAN, número, pai e posição, não as `PgnGame` com um tabuleiro por nó), memorizado por hash do PGN e com teto pela memória estimada (≈ 250 mil lances) e aviso.
- *Tarefas:* autosave: (1) apagar a releitura de cada partida; enquanto o arquivo se chamar `*.games` (até o G3b), o `StoredComments` continua gravado, sem releitura — todos os `comment_before` e todo comentário acima de 4 KiB, que é o que o leitor dos builds instalados perde (A1-02, A1-06; R9-01); ele sai no `<hash>.games.json` do G3b; (2) medir; (3) só se ainda passar de 5 ms, cache por revisão e gravação em worker; `page_fingerprint` recalculado só para as páginas que o `chess_carry` diz terem mudado; jobs da detecção montados no worker; PGN do livro indexado no worker, carregado uma vez por documento; `BookPgnView` com `Arc`.
  Se houver worker, as regras dele: um thread gravador por store com uma vaga "último pedido" de geração crescente (uma gravação mais velha nunca acontece depois de uma mais nova, e nunca há duas do mesmo arquivo em voo); Fechar e Sair esperam a gravação pendente até 2 s, e passado isso perguntam, como o `first_dirty`; o erro volta ao thread da UI como `Blocked` (o aviso de hoje).
- *Verificação:* contador no `Store::Memory`: mudar 1 de 120 partidas recodifica 1; editar 1 página depois de varrer recalcula 1 impressão **e** 1 `page_key`, e mudar o Título recalcula 0 de cada; PGN de referência no teto (999.847 lances) com pico ≤ 300 MB, fora do thread da UI; teste `#[ignore]` de tempo: quadro do autosave ≤ 5 ms com 120 partidas; backend falso lento, mudanças 1, 2 e 3 em 10 ms → o disco termina com a 3 em ≤ 2 gravações; Sair 100 ms depois de uma mudança → a mudança está no arquivo.
- *Esforço:* M.
- *Reversão:* o `.games` v1 continua com o `StoredComments`; revert sem efeito no disco.

**A9-I4 — Pedidos pendentes e posição efetiva.** Achados A5-04, A5-05, A5-17, A5-09 (parte final).
- *Tarefas:* `pick_after_scan`; "Diagrama N" varre a página do texto e abre a aba Diagrama; "Continuar editando" de um rascunho de outro documento usa o mesmo pedido; linha, menu e tabuleiro leem a posição efetiva do `Corrected.applied` do motor (G2), sem regra própria; "Revisar agora" limpa o filtro.
- *Verificação:* kittest com `run_inline = false`: texto da pág. 5, vista na 9, clique → editor em (5,0); linha Corrigida → `OpenLichess` com o FEN guardado; diagrama sem leitura e com correção → "Abrir no tabuleiro" habilitado.
- *Esforço:* S–M.

**A9-I5 — Razões do motor tipadas e traduzidas.** Achados A5-13, A6-08.
- *Tarefas:* `PoorReason`, `ScanNote`, `Unreadable` como enums com dados; `chess_widgets::reason_text`; ferramentas headless continuam com o texto inglês derivado do enum.
- *Verificação:* para cada variante, o texto pt-BR difere do inglês e está no catálogo.
- *Esforço:* M.

**A9-I6 — Duplicações pequenas.** Achados A5-19, A7-19 (o A5-16 está em G8).
- *Tarefas:* `move_status_words`; `BoardFrame { rect, flipped }` com `square_at`/`square_rect` para os dois tabuleiros e as setas; `enum BoardContent { Game(Game), Illegal { placement, fen } }` no lugar de `game` + `illegal: Option<…>` (`chess_board_ui.rs:175-177`), que deixa representar partida com lances e posição ilegal ao mesmo tempo; editor sobre `History<Position>`; `pdfcraft_chess::legal_moves`; `pdfcraft_chess::hash::fnv64`.
- *Verificação:* `square_at(square_rect(sq).center()) == sq` para as 64 casas nas duas orientações, nos dois tabuleiros.
- *Esforço:* S.

**A9-I7 — Oferta do PGN irmão** (feito dentro do I15, na onda 1). Achados A6-02, A6-03.
- *Tarefas:* `sibling_pgn` pura com nomes normalizados (sem símbolos/emoji, espaços colapsados, sem caixa) e, na falta, o único `.pgn` da pasta como oferta (**I7a**, dentro do I15: só a comparação por nome normalizado; **I7b**, depois do I2: o "único `.pgn` da pasta" num `Job<T>`, com teto de entradas — a pasta pode ser Downloads); botões curtos "Usar" e "Agora não", nome do arquivo no texto que quebra linha e no nome acessível.
- *Verificação:* "📚Livro.pdf" + "Livro.pgn" → oferta; painel de 330 pt e nome de 80 caracteres → "Agora não" dentro do painel.
- *Esforço:* S.

**A9-I8 — Leiaute que mede antes de desenhar (o que sobra depois do I0).** Achados A6-05, A6-13, A6-15, A6-17 (barra), R4-07.
- *Tarefas:* §2.8.1 "Medir antes de desenhar" na aba Diagrama (dica e linha reservada do aviso), na linha da lista (estado como ícone + palavra curta), nos `ScrollArea` do painel e na barra de progresso (`widgets::progress`); tabela "um ícone, um conceito" com teste; `clippy.toml` com `egui::Modal::new` e `egui::ProgressBar::new` proibidos.
- *Verificação:* kittest: o aviso "Alterações não confirmadas" não muda o `y` do tabuleiro (falha antes: +24 pt; o "grade dentro da aba" já é o teste do #254); painel estreito → retângulos da linha sem sobreposição; capturas a 1360 × 705 e 1440 × 900.
- *Esforço:* S–M.

**A9-I9 — Aba Texto: um parágrafo, um galley.** Achados A6-06, A6-07, A6-23, R5-12.
- *Tarefas:* §2.8.1 "Aba Texto — dois eixos": `LayoutJob` por parágrafo, cor de texto por token + marcador de forma pintado para o estado, sublinhado contínuo (o de hoje) para a ligação ao PGN; os seis controles do topo vão para o ⋯ (esboço c), com "Remover PGN do livro" por último e em `danger_text`.
- *Verificação:* contraste de todos os estados ≥ 4,5:1 nos dois temas; focáveis num `PageText` sintético ≤ lances + diagramas; nome do lance "6. Bispo b5, xeque, não conferido, lido do PGN"; entrelinha uniforme (altura das linhas de um parágrafo igual); um lance ilegal tem o marcador "×" (falha antes: só cor).
- *Esforço:* M.

**A9-I10 — Notação única.** Achados A6-09, R5-09.
- *Tarefas:* `move_text(game, node, notation)` sobre `San::write` em todas as superfícies (lista, trilha, marcador, captura, linhas do motor, nome falado); `chess_figurines::letter`/`in_letters` apagados; `spoken` no `pdfcraft_chess`; a prévia de lance com foco do teclado fecha com Esc sem mover o foco (WCAG 1.4.13, `chess_board_ui.rs:1792-1823`).
- *Verificação:* com "Letras no seu idioma", trilha e linhas do motor dizem "Cf3".
- *Esforço:* S–M.

**A9-I11 — Acessibilidade do painel.** Achados A6-10 (modal), A6-11, A6-17, A6-18, A6-20, R5-07, R5-08. Dois PRs: **I11a** (onda 2, antes do I0: modais, `labelled_by`, papéis `Menu`/`MenuItem`, contraste não-texto; e o roteiro do NVDA com o texto esperado por passo, em `docs/chess/analise-9/d26-roteiro.md`, com uma parte para o EPUB que o X3 completa — a D26 roda pela primeira vez no fim do X3, onda 3) e **I11b** (onda 5, depois do I0a: abas, mestre-detalhe, foco; acrescenta ao roteiro os passos da coluna Xadrez).
- *Tarefas:* os itens de §2.8.1 "Acessibilidade" e "Cores" que o I15 não fez, um por um: modais completos (`modal`, Tab preso, Esc, foco de volta); foco no mestre-detalhe; `labelled_by` em todo controle; papéis `Menu`/`MenuItem`; contraste não-texto; usar os testes de tokens que já existem (`theme.rs:786-825`, `contrast()`) em vez de criar outros; as linhas da matriz §2.8.3 marcadas com este passo.
- *Verificação:* teste genérico sem nó focável sem nome (percorre também a barra do documento, onde está a caixa da página); modal com papel `Dialog`, `modal`, foco no 1º controle não destrutivo, Esc fecha e o foco volta; abas com papel Tab; `click` por AccessKit fecha o menu; contraste não-texto ≥ 3:1; com a janela **em uso**, Tab por todos os focáveis do painel e cada um com anel ≥ 2 pt e ≥ 3:1 contra o vizinho (a WCAG 2.4.7 vale com a janela em uso; o A6-20 era efeito da janela sem foco).
- *Esforço:* M.

**A9-I12** — absorvido pelo I0 (I0b e I0c).

**A9-I13 — O motor achado e encerrado.** Achados A6-10 (motor), A4-19. (O "Original / Lido" foi para o I0.)
- *Tarefas:* ferramenta `chess_diagram_crop` (a região do diagrama renderizada sob demanda, para agentes; o I0 desenha o Original por `RenderRequest.tile` e não depende dela); a busca do motor em subpastas já existe (#254: qualquer programa, três níveis, 256 entradas; nenhuma regra de nome) — o assistente passa a dizer **qual** programa achou e onde (com dois candidatos, o primeiro por nome vence em silêncio); um programa que não fala UCI derruba o `Analyser` (A4-19); no Sair, o `Drop` do `Analyser` mata o filho de forma síncrona: `kill()` (imediato), depois `try_wait` em laço até 500 ms, depois desiste (`Child::wait` não tem prazo; hoje o `Drop` faz isso num thread solto, `analysis.rs:353-372`, que morre com o processo — a causa do A4-19). Job Object só por uma dependência de API segura, com licença conferida (o workspace proíbe `unsafe`, `Cargo.toml:85`); a queda do app com o motor vivo fica como limite no parity.
- *Verificação:* um programa que não fala UCI é encerrado (falha antes: continua rodando); Sair com o motor analisando não deixa processo filho (falha antes); com `engine/aux/x.exe` e `engine/stockfish/stockfish.exe`, o assistente nomeia o achado.
- *Esforço:* M.

**A9-I14 — Textos, glossário e Preferências.** Achados A6-16, A6-21, A6-22, A6-26, R4-11, R10-06.
- *Tarefas:* o que sobrar da tabela §2.8.2 depois do I15; teste de glossário sobre `pt-br.tsv` e sobre o texto-fonte inglês (§2.1, §2.8.2), **por radical** e não por frase (`procur`, `busca`, `varre` em textos sobre diagramas; "glifo" fora de "modelo de glifos"); os 34 textos de xadrez que faltam em `el`, `pl`, `te` e `zh-hant`; Preferências com botão Fechar e índice lateral de seções; conjuntos de peças com nomes traduzidos; um só indicador "Lendo a página…"; números com o locale.
- *Verificação:* teste de glossário, que falha hoje em 2 + 2 textos (R10-06); os 22 catálogos com os 870 textos de xadrez; captura de Preferências a 1366 × 768 em que a seção de xadrez está a **um clique** no índice (medido: o retângulo do título da seção fica visível depois de um clique, sem arrastar a barra).
- *Esforço:* S + M (índice).

#### Onda E — entrega e documentação

**A9-E0 — Entrega ao fim de cada onda** (S por onda; decisão D28). Achado R10-01 (parte), §2.10 ("notas da versão").
- *Tarefas:* (1) refazer o `dist/app` do `main` (release, `CARGO_TARGET_DIR` próprio, um build por vez), guardando o anterior em `dist/app/anterior-<data>/` **completo** — com `models/`, `fonts/` e `engine/` (hoje o `anterior-2026-10-31-1428` tem só os dois `.exe`, e `chess_status` responde `pieces: missing`: ele não lê diagramas); (2) notas da versão em pt-BR em `docs/chess/notas/<data>.md`, tiradas da coluna "Formato gravado" da ficha e dos PRs da onda: o que mudou para o usuário, o que um build anterior deixa de ver ou de gravar (correções v2, `<hash>.<kind>.json`, `.games.json`, preferências novas) e o que fazer; (3) a ajuda e o guia: o que entrou em `docs/chess/ajuda/PENDENTE.md` na onda.
- *Verificação:* o `pdfcraft-cli --version` dos dois builds diz o build (E2); `chess_status` do novo e do anterior dizem `pieces: loaded`; as notas citam todo formato gravado da onda.
- *Esforço:* S por onda (não é PR de código; o PR é só das notas).

**A9-E1 — Ajuda em PDF e guia refeitos** (onda 6, faixa B, depois do N7b; S–M). Achado R10-03.
- *Tarefas:* reescrever os capítulos da ajuda (pt-BR, 36 pp.) que as ondas mudaram — a coluna Xadrez do I0, os textos de §2.8.2, o menu Xadrez e os atalhos (I16, I18), "sinal" no lugar de "glifo" para NAG (I14), o motor numa pasta dentro de `engine/` (#254) —, a partir da fonte guardada no Passo 0, com capturas pelo canal de controle numa cópia portátil (`portable.txt` + `--control`) do build da onda; `docs/chess.md` (inglês) com os mesmos capítulos; o teste de glossário do I14 lê também a fonte da ajuda.
- *Verificação:* nenhuma palavra proibida de §2.8.2 na fonte da ajuda (falha antes: 19 rótulos e 11 "glifos"); cada captura com o build na legenda; `PENDENTE.md` vazio.
- *Esforço:* S–M.

**A9-E2 — Builds lado a lado e atualização** (onda 0, faixa A; S). Achados A6-27, R10-01, R10-02.
- *Tarefas:* (1) o nome do socket de instância única leva um hash curto da pasta do executável para **toda** cópia, instalada ou portátil (hoje só a portátil ganha nome próprio, `apps/pdfcraft/src/single_instance.rs:155-163`, e o `dist/app` e o `anterior-…` não são portáteis: abrir um PDF pelo antigo com o novo aberto entrega o arquivo ao novo); (2) um id de build (commit curto + data, posto pelo `build.rs` a partir de uma variável do roteiro de entrega; "dev" sem ela) no Sobre, no `--version` e na resposta de `chess_status`; (3) "Verificar atualizações" do fork não oferece a versão do PdfCraft original (`storytold/pdfcraft`, sem as ferramentas de xadrez): diz "PdfCraft Xadrez <build> — as versões novas vêm do `dist/app`", sem rede.
- *Verificação:* dois executáveis em pastas diferentes → nomes de socket diferentes, o mesmo executável → o mesmo (teste de `socket_name`); `--version` com o id do build; o diálogo de atualização no fork não chama a rede (teste com a fonte de rede falsa).
- *Esforço:* S.

#### Onda N — novas funcionalidades

**A9-N1 — Lance clicável no PDF** (MVP). A8-02. §2.9. Depende de I2, I9 e I0. *Design:* o duplo clique na página já tem dois sentidos (seleciona a palavra; põe no tabuleiro o diagrama de página não lida, recusando em silêncio quando pegou uma palavra — `chess_ui.rs:197-205,778-781`, `canvas.rs:3634-3650`): em lance de página **lida**, o duplo clique abre a partida **e** deixa a palavra selecionada (inofensivo); em página não lida, lê a página primeiro, com o aviso "Lendo a página…" na base da página; ao passar o ponteiro sobre um lance lido, cursor de mão e sublinhado de 1 pt em `accent` (a affordance; sem ela ninguém descobre); Alt+clique faz o mesmo sem selecionar; no modo Ler funciona igual. *Verificação:* kittest com página sintética de um lance: duplo clique sobre ele põe no tabuleiro a FEN depois do lance (falha antes); duplo clique em prosa não faz nada; `chess_move_at`. *Saída:* jornada "ver um lance impresso no tabuleiro" de ≥ 3 gestos para 1 (medida no teste de jornadas). *Esforço:* S–M.

**A9-N2 — Uma verdade das partidas para todas as exportações** (MVP). A8-03. §2.9. Depende de F4 e G2b. *Verificação:* uma página sintética com um lance ilegal e um PGN com o certo → o HTML exportado tem o `data-fen` depois do lance, marcado; Harikrishna com o PGN do livro: lances válidos do EPUB = os do PGN exportado (± 1 %). *Esforço:* M.

**A9-N3 — Esta posição no livro** (MVP). A8-07. §2.9. Depende de T1a, I0c e I3b (o índice compacto). *Orçamento:* usa o mesmo índice do PGN do livro (compacto, I3) ou a leitura que **já está** na sessão (G11) ou no N5 — "Procurando a posição no livro…" **nunca** dispara a leitura do livro inteiro (4,7–12 min); memória ≤ 64 B por posição com `BookKey` compacto. *Design:* um item "No livro (n)" na barra inferior do Tabuleiro (esboço d), que abre a lista no lugar dos lances (Esc volta); estados: carregando "Procurando a posição no livro…", vazio "Esta posição não aparece no livro lido.", sem leitura "Leia o livro (Detectar diagramas ou Texto) para procurar". *Verificação:* livro sintético com a mesma posição por duas ordens de lances → 2 ocorrências e a transposição contada; `chess_position_find`; lista navegável pelo teclado. *Esforço:* M.

**A9-N4 — Solucionador de mate em N, com as convenções de problema.** A8-04, R6 (O1). Depende de L6c (o `Stipulation`).
- *Tarefas:* `chess/src/mate.rs` — `solve(fen, n, budget) -> MateReport { keys, shortest, nodes, exhausted }`, n ≤ 3 no MVP (mate em 4 só com orçamento medido), puro, wasm; roda **só** em `Mate(n)` direto (ajudamate, automate e estudos de ganhar/empatar nunca recebem `NoMate`); o **lado** é decidido pelo solucionador só quando exatamente um lado dá mate em ≤ n — quando os dois dão (o padrão de toda combinação na última fileira), fica `Assumed` com a nota "os dois lados dão mate em n"; **roque e en passant pela convenção de problemas** (roque permitido salvo prova em contrário: os direitos inferidos pelas peças, que a leitura já usa; en passant só quando a legenda o prova, `passed_square`), e o relatório diz quando a chave usa roque ("supõe o roque permitido"); mais de uma chave num "#n" é **solução múltipla** (*cook*) e vira marca de revisão (provável leitura errada), mate mais curto que o pedido é informação, e "dual" sai do MVP; `ReviewFlag::NoMate { n, best_try, refutation }` (a melhor tentativa e a defesa que a refuta: o que deixa o usuário decidir entre "li errado" e "o livro errou"; o mesmo relatório "o livro diz × o cálculo diz" do N17); aba Diagrama "Mate em 2: 1.Df7!" com "(única)" só com chave única; `chess_mate`.
- *Verificação:* `3r2k1/5ppp/8/8/8/8/5PPP/3R2K1` → os dois lados dão mate em 1, lado fica `Assumed`; `8/8/8/8/4R3/6k1/8/4K2R w K - 0 1` → chave única `O-O`, e com `-` sem mate em 2; as 12 FEN do Polgar guardadas (`pol.json`) → brancas em 12, chave única em 10, 1 sem mate e 1 com solução múltipla (14 chaves), os dois para revisão; um "h#2" sem `NoMate`; proptest sem pânico no orçamento. Custo medido pela R6: 13 posições × 2 lados com n ≤ 2 em < 10 ms (release).
- *Orçamento:* em nós por diagrama (`budget`), com o tempo por nó medido; roda num job, nunca no quadro (no wasm, por `step`); em lote, 1.000 diagramas × 2 lados ≈ 20 s se valerem os 10 ms por posição do A8-04 — dito na interface antes de começar.
- *Esforço:* M.

**A9-N5 — Leitura guardada em disco** (resolve a **reabertura**; dentro da sessão, o G11 já evita reler). A8-06, R3-10. *Orçamento:* o documento lido inteiro em JSON tem 19,9 MB no Harikrishna (26,6 KB por página; medir o estágio local antes): ≈ 20 MB por livro, ≈ 39 MB no *Chess Evolution*, ≈ 350 MB para os 38 livros sem compressão — cabe nos 512 MiB da D21 com pouca folga, então **comprimir** (o `deflate` já está no `export`) é o padrão; a chave da página é um só hash de conteúdo por objeto e por estado, compartilhado com a `page_key` (não um terceiro passe de SHA-256 além de `page_key` e `page_fingerprint`); leitura por página sob demanda, nunca o arquivo do livro inteiro na abertura. Um `StoreKind` "reading" (G3) por documento; guarda o **estágio local** da página (antes da cifra e do reparo, que dependem de outras páginas e rodam depois, sobre as páginas guardadas — A9-L0/V4); chave da página = SHA-256 do conteúdo + identidade dos modelos + configurações + `READER_VERSION`; teto total (D21) que também respeita o espaço livre (para de guardar abaixo de 1 GB); cada entrada leva a própria chave e é conferida na leitura (diferente = falta, nunca servida), com gravação atômica por entrada; o despejo tolera `NotFound` e um 2º processo; segue a `persist_policy` (o cache guarda o **texto** das páginas: PDF cifrado só com consentimento); `cache: DIR` nas ferramentas, `chess_cache_status/clear`. Dois PRs: **N5a** (o `StoreKind` "reading", a chave, o teto, a conferência por entrada e o despejo) e **N5b** (as ferramentas, a `persist_policy` do PDF cifrado e o uso pelo G11 e pelo N3). *Verificação:* ler, reabrir → zero páginas relidas; trocar o modelo → todas relidas; uma entrada corrompida é relida, não servida; um PDF cifrado não deixa arquivo `reading` antes do "Sim". *Esforço:* M–L.
- *Reversão:* o `reading` é cache: um build revertido não o lê, e `chess_cache_clear` o apaga; revert.

**A9-N6 — Marcadores do livro.** A8-13. Depende de L1b (os papéis dos cabeçalhos) e G11. Capítulos ▸ Partidas ▸ Exercícios sob um marcador "Xadrez", prévia antes de aplicar, um passo de desfazer, índice de partidas em CSV/Markdown, `chess_bookmarks {dry_run}`. *Verificação:* `chess_bookmarks {dry_run}` num livro sintético com 2 capítulos e 3 partidas → a árvore esperada; aplicar e desfazer → o PDF byte a byte igual. *Esforço:* S–M.
- *Reversão:* os marcadores aplicados são um passo de desfazer; revert.

**A9-N7a — Lance pelo teclado e tabuleiro descrito** (nível A; logo depois do I17, de que usa o `announce`, e do I10, de que usa os nomes falados — onda 5, à frente de toda a onda N). R5-03. (1) Campo "Lance" sob o tabuleiro, um foco só: aceita SAN no idioma da interface e em inglês (`San::read`), UCI (`e2e4`) e comandos no padrão do modo cego do lichess (`p` peças, `u` último lance, `f e` coluna, `r 4` fileira, `s` quem joga), com a resposta no `announce` (I17) e lance ilegal como `Alert` com o motivo; (2) — o nome do tabuleiro pela descrição já foi feito no I15 (7); (3) setas e círculos também pelo campo (`seta e2e4 verde`, `círculo d5`) e por Alt+clique na origem e no destino (WCAG 2.5.7). O texto do campo **nunca** é repassado ao motor UCI: só o lance já conferido pela legalidade entra na partida. *Verificação:* digitar "Cf3" → a partida tem Nf3 e o anúncio diz "Cavalo f3"; "Cf4" → `Alert` "lance ilegal"; "p" → o anúncio lista as peças; "seta e2e4" → o PGN tem `%cal Ge2e4`. *Esforço:* S–M.

**A9-N7b — Tabuleiro de análise navegável casa a casa.** A8-18, A6-25. A grade de casas do editor reaproveitada no tabuleiro de análise (valor = casa em foco), lance jogado anunciado com o nome falado de I10, teclas de consulta com o prefixo **Alt+Shift+** (P peças, 1–8 fileira, A–H coluna; anúncio por status), que não colidem com as do trilho (B, C, F, G, O, X) nem com as letras de peça do editor nos 18 idiomas — teste de colisão sobre o mapa de teclas. *Verificação:* kittest só com teclado: Alt+Shift+P anuncia as peças e a grade tem 64 células com `bounds` (falha antes: imagem com a FEN). Depende de I0c, I10 e I11b. *Esforço:* S–M.

**A9-N8 — Opções e histórico do motor.** A8-11, R6-09. Threads, Hash e SyzygyPath enviados só se o motor os anunciou — com o nome da opção idêntico ao anunciado, números limitados ao `min`/`max` anunciados, `MAX_OPTIONS` para as anunciadas e valores **sem CR/LF/controle** (um `SyzygyPath` com `\nquit` é recusado: injeção de comando UCI); `position fen <início> moves …` com os lances desde o último lance irreversível (no máximo 100 meios-lances, pelo relógio), para o motor ver repetição e xeque perpétuo (hoje só a FEN, `analysis.rs:430`) — o cache `kept` passa a ser chaveado por `Fen::full()` + histórico; avaliações guardadas por posição (um `StoreKind` "evals"). *Verificação:* o motor falso recebe `setoption name Threads` só quando anunciou. *Esforço:* S–M.
- *Reversão:* o `evals` é novo: um build revertido não o lê; revert.

**A9-N9 — Das partidas a um livro.** A8-12. `doc::from_games(&[Game], opts) -> ChessDoc` e os exportadores existentes; "Exportar como livro ▸ Word/EPUB/…" na lista de partidas. *Verificação:* `doc::from_games` de 2 partidas com `$220` → EPUB com 2 diagramas, e o epubcheck sem erro quando instalado. *Esforço:* M.

**A9-N10 — Exercícios para treinar fora do app.** A8-05, D20. Primeiro o **PGN de problemas padrão**, que todo programa lê: `SetUp`/`FEN`, a solução como linha principal, `Result` e o enunciado num comentário antes do 1º lance — serve ao "Replay training" do ChessBase, a estudos do lichess, a cursos privados do Chessable e aos complementos do Anki (TSV + PNG). O `%tqu` do ChessBase fica como opção **depois** do teste da D20: não há documento oficial e as duas fontes públicas dão formas diferentes (a do A8 tem campo de idioma; a da página jberries não); o treino do CB só vale num banco com o atributo "Training". *Verificação:* PGN de 3 exercícios com `SetUp`/`FEN`, relido pelo python-chess (processo externo) → a mesma FEN e a solução como linha principal. *Esforço:* S–M.

**A9-N19 — Lista de erratas da notação** (o que um editor pede primeiro; S–M). R6 (Recomendado). Para cada lance impresso: marca de captura que não confere e `+`/`#` a mais (L5); número fora de sequência; conflito com o PGN do livro numa página **nascida digital**, decidido pelo PGN (não pode ser erro de leitura, então é erro do livro); depois, com o N17, "!" que o motor refuta. CSV/Markdown com página e lance; ferramenta `chess_errata`. Os dados já existem. *Verificação:* página sintética com "Nxd8" para casa vazia e um "+" a mais → 2 linhas no CSV, com página e lance.

**A9-N11 a N18** — N11 estudo do lichess (A8-16) · N12 folha de exercícios (A8-14) · N13 **vago** (era o pacote de revisão, hoje G5b) · N14 **cortado** (A8-17) · N15 biblioteca (A8-08, D22) · N16 padrões (A8-09) · N17 motor em lote (A8-10) · N18 modo resolver (A8-15, D23). Em detalhe: estudo do lichess (A8-16; usa o dialeto Lichess do F7), folha de exercícios (A8-14), ~~lances como links invisíveis no PDF (A8-17)~~ — **cortado**: com o N1 no app sobra pouco valor, e o alvo minúsculo é ruim nos leitores externos —, biblioteca (A8-08, D22; custa **horas**, não "L": pelas taxas medidas, 38 livros de ~350 pp., metade escaneada, dão ≈ 4 h num núcleo e ≈ 1,2 h com o V10 — exige G11, V10 e N5, indexa em segundo plano, retomável por livro (idempotente), com teto de núcleos e pausa; ao varrer pastas, nunca abrir arquivos-espaço reservado de nuvem — OneDrive "sob demanda", `FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS`/`OFFLINE`, que baixariam o arquivo —, nunca seguir junções nem links, teto de entradas e de profundidade, e o índice sob a `persist_policy`), padrões (A8-09), motor em lote via ChessBase e depois no app (A8-10, que compartilha o relatório "o livro diz × o cálculo diz" com o N4), modo resolver (A8-15, D23). O N6 (marcadores) ganha também o **índice de variantes** no estilo Quality Chess (lances → página), gerado do índice do N3 — o apêndice que um tradutor precisa recompor. Cada um ganha especificação própria quando a decisão correspondente for tomada; os contratos de partida estão em `analise-9/A8.md`.

### 3.4 Ficha de cada passo

Onda e faixa da §3.2 · dependências da §3.1 · PRs e esforço · **formato gravado** (quando o passo muda um arquivo que outro build lê: exige a Reversão própria e o teste de compatibilidade de §2.10).

| Passo | Onda · faixa | Depende de | PRs · esforço | Formato gravado |
|---|---|---|---|---|
| P | P · A | — | 2 · S | — |
| Q1a · Q1b | 0 · A · 1 · A | P · Q2 | 1 · M · 1 · S–M | `perf-baseline.json` (local) |
| Q2 | 1 · A | Q1a | 1 · S–M | bases locais dos benchs |
| Q3a · Q3b | 4 · C | Q1b · F2b, A9-L0a | 1 · M · 1 · S | — |
| Q4a · Q4b | 1 · C · 4 · C | Q1a | 1 · S · 1 · S–M | — |
| Q5 | 4 · C | I18 | 1 · S–M | — |
| T1a · T1b | 2 · C · 3 · B | Q1a · T1a, I1b | 1 · M · 1 · M | — |
| T2a · T2b | 2 · C · 3 · B | T1a · T2a, I1b | 1 · M · 1 · S–M | `Decision` sem mudar o JSON (`#[serde(flatten)]`, no G4) |
| T3 | 2 · A | Q1a | 1 · S–M | — |
| G1 | 0 · A | Q1a | 1 · S | `/ID` dos PDFs novos; stores misturados separados |
| G2a · G2b · G2c | 1 · A · 2 · A+B · 5 · A | Q1a · G2a, T1a · G3b, T2a | 1 · S–M · 2 · M · 1 · S–M | corrections v2 · — · âncoras das partidas |
| G3a · G3b · G3c | 2 · A · 5 · A · 5 · A | G2b, T3 · G3a, I3b, G12 · G3a, I3b, V4a | 2 · M · 1 · M · 1 · S | `<hash>.<kind>.json`, reference v2 · `<hash>.games.json` · cifra |
| G4 | 3 · A | G3a | 1 · S | reference v2 (`pgn_san`, `bytes_hash`) |
| G5a · G5b | 3 · A · 5 · A | G3a, T3 · G3b, G5a | 1 · M · 1 · M | `migrated.json` · pacote de revisão |
| G6 | 2 · A | T3 | 1 · S | — |
| G7a · G7b | 0 · A · 3 · C | Q1a · A9-L0b | 1 · S · 1 · S | — |
| G8 | 0 · B | G12 | 1 · M | — |
| G9 | 5 · C | L6b, I10 | 1 · M | links no PDF |
| G10 | 3 · B | G3a | 1 · M | — |
| G11 | 1 · A + B | Q1a | 2 · S–M | — |
| G12 | 0 · B | Q1a | 1 · S | política de versão do `.games` |
| F1 | 0 · C | Q1a | 1 · S–M | — (aceita o hash legado) |
| F2a · F2b | 1 · C | F1 · F2a | 2 · M | — |
| F3 | 1 · C | F2b | 1 · M | — |
| F4 | 3 · A | F3 | 1 · M | PGN escrito em 79 colunas |
| F5 · F6 | 3 · A | F2b | 2 · M · 1 · M | — |
| F7 | 3 · A | F4, D18 | 1 · S–M | — |
| A9-L0a · A9-L0b | 2 · C | T1a · A9-L0a | 1 · M · 1 · M | `READER_VERSION` · decisões por `san_read` |
| L1a · L1b | 3 · C | A9-L0b · L1a, G3a | 1 · M · 1 · M | `role` no JSON do documento · `reader_version` |
| L2 · L3 · L5 · L7 · L8 | 3 · C | A9-L0b | 1 · S–M cada | — |
| L4 | 5 · C | Q2, A9-L0b | 1 · S | — |
| L6a · L6b · L6c | 3 · C | A9-L0b · L6a · L6b | 1 · S · 1 · M · 1 · S–M | — |
| L9 | 5 · C | L4 | 2 · L | — |
| V1 · V8 | 0 · C | Q1a · V1 | 1 · S–M · 1 · S–M | — |
| V2 · V3 | 4 · A | Q1b · V2 | 1 · M · 1 · M | — |
| V4a · V4b | 4 · A · 5 · A | — · G3c | 1 · S · (no G3c) | — · cifra |
| V5 · V7 · V9 | 4 · A | — | 1 · S · 2 · S · 1 · S | — |
| V6a · V6b | 1 · C · 5 · B | — · I2 | 1 · S · 1 · M | conjunto de peças (troca atômica) |
| V10a · V10b | 4 · A · 5 · B | Q1b · V10a, I2 | 1 · M · 1 · S | — |
| X1a · X1b | 3 · C · 4 · C | — · D18 | 1 · S · 1 · M | — |
| X2 | 5 · B | I17 | 1 · M | — |
| X3 | 3 · A | L6b | 1 · S–M | — |
| I15 (com I7a) | 1 · B | Q1a | 2 · S–M | — |
| I18 | 2 · B | Q1a | 1 · S–M | preferências (preserva chaves desconhecidas) |
| I1a · I1b | 2 · B · 3 · B | G2b · I1a | 1 · L · 1 · M | — · backend web |
| I11a · I11b | 2 · B · 5 · B | I1a · I0a, I11a | 1 · M · 1 · M | — |
| I2 | 3 · B | I1b | 2 · M | — |
| I3a · I3b | 4 · B · 5 · A | F4, G12 · I2 | 1 · M · 1 · M | `.games` v1 (mantém o `StoredComments`) · — |
| I0a · I0b · I0c | 4 · B | I1b · I0a · I0b | 3 · L | — |
| I4 · I8 | 4 · B | I2 · I0c | 1 · S–M · 1 · S–M | — |
| I9 · I10 · I17 | 5 · B | I0c · I9 · I10, I0c | 1 · M · 1 · S–M · 2 · M | — |
| I13 · I5 | 5 · A | T3, I0c · — | 1 · M · 1 · M | — |
| I14 · I16 · I6 · I7b | 5 · B | I15, I0c · I18, I0c · — · I2 | 2 · S + M · 1 · S · 1 · S · 1 · S | — |
| N7a | 5 · B | I10, I17 | 1 · S–M | — |
| N1 · N3 · N7b | 6 · B | I0c, I2, I9 · T1a, I0c, I3b · I0c, I10, I11b | 1 · S–M · 1 · M · 1 · S–M | — |
| N2 · N9 · N10 | 6 · C | F4, G2b · F4 · F4, D20 | 1 · M · 1 · M · 1 · S–M | — |
| N4 | 4 · C | L6c | 1 · M | — |
| N5a · N5b · N6 · N8 | 6 · A | G3a, G11 · N5a · L1b, G11 · — | 1 · M · 1 · M · 1 · S–M · 1 · S–M | `reading` (novo) · · marcadores no PDF · `evals` (novo) |
| N19 | 5 · C | L5 | 1 · S–M | — |
| E0 | fim de cada onda · A | a onda | 1 por onda · S | notas da versão |
| E1 | 6 · B | N7b | 1 · S–M | — |
| E2 | 0 · A | Q1a | 1 · S | socket por pasta; id do build |
| N11–N18 | 6+ | D20–D23 (o N15 também G11, V10a, N5b) | — | — |

## 4. Invariantes após todo passo

`cargo xtask chess-gate` (Q1) verde, que junta: `cargo fmt --check`; clippy `-D warnings` dos crates tocados (incluindo `-p pdfcraft-cli -p pdfcraft` quando um tipo público mudar); testes dos crates tocados (o crate inteiro, conferindo o número de testes rodados); `cargo check` wasm32 de `chess`, `chessvision`, `engine`, `automation`; `xtask layers`, `assets`, `parity` e `cargo test -p xtask`; i18n (18 catálogos + SHA-256); arquivos em LF. Fora do portão, por passo: teste novo visto falhando com a correção revertida; ferramenta headless e entrada em `parity/` para função nova; capturas antes/depois no PR para o que for visível; benchs de Q2 sem perda quando tocar o walker ou a visão.

## 5. Métricas de aceitação

**Como medir.** Toda meta de tempo é medida em **release**, na máquina de referência (Ryzen 5 8400F, 6 núcleos/12 threads, Windows 11), pelo comando escrito no passo, e roda na etapa 4b "desempenho" do Q1 (`perf_*`): cada teste `perf_*` afirma primeiro uma **contagem determinística** (chamadas ao modelo, páginas rehash, partidas recodificadas, comparações), que falha sempre que quebra, e depois compara o tempo a `cargo-targets/gate/perf-baseline.json` (gerado do `main` do dia, na mesma máquina; falha acima de 2× a base; o resumo diz a máquina e o perfil). Os números "perfil de teste" de A3 viram metas em release.

| Medida | Hoje | Meta | Passo |
|---|---|---|---|
| Perdas e resultados errados silenciosos, C e I: perda (A1-01, A1-02, A1-05, A1-10, A1-12, A2-01, A2-07, A4-05, A4-06, A4-07, A4-08, A4-11, A4-13, A5-01ᴸ, A5-07ᴸ, A6-04, R1-02ᴸ) · resultado errado (A1-03, A2-10, A4-01, R1-01, A4-02, A4-03, A6-01, A3-04) · aborto (A3-01, R2-01, R7-01) · perda na troca de peças (R7-02) | 29 (ᴸ = só por leitura) | 0, cada um com teste de regressão visto falhando | núcleo (18 itens); G3a, I1a, L2, V6a (24); G4, G10, L5, L7, V4a |
| Round trip de PGN do corpus (próprio leitor e python-chess) | não medido para BOM/Latin-1/comentário longo/variante repetida | 100 %, com a propriedade-guia de §2.5 | F1, F2b, F3, F4 |
| Lances sem número depois de prosa, sem ligação ao PGN do livro (`absent`/`conflict`; Harikrishna 60 pp.) | 39 (5 jogados como válidos, 24 ilegais em cascata, 10 outros) | 0 — e os 2 confirmados e os 15 recuperados continuam lances | L4 (medido pelo Q2) |
| Texto perdido entre diagrama e lance (dv100, fi30, flores, ce60) | 36 trechos, ~5,4 mil caracteres | 0 | L2 |
| Parágrafos partidos no meio da frase (Harikrishna 60 pp.) | 105 de 619 | ≤ 10 | L3 |
| 38 livros (`chess-bench`) | 25.656 válidos (#232) | ≥ base do dia, `--allow-lost 0` | Q1b (etapa 5); todo passo de L e V |
| Quadro do autosave com 120 partidas · com **uma** partida de 10.000 lances | 78–300 ms no thread da UI · 70,7 ms só no `record` | ≤ 5 ms · ≤ 5 ms | I3a |
| Edição depois de varrer o livro (748 pp.): Título · uma página | 748 páginas rehash (≈ 33 ms) + 748 impressões (≈ 66 ms) | 0 · 1 página rehash e 1 impressão; quadro ≤ 16 ms | G2b, I3b |
| Ler o livro inteiro (Harikrishna; detectar + ler) | 4,7 min na CLI (1 núcleo), ~7 min no app | ≤ 1,5 min (V10) | V10a, V10b |
| Páginas relidas por Exportar, Ligar o livro e Ler a página logo depois de "Detectar no livro" | 748 / 748 / 2 por leitura | 0 (G11) | G11 |
| Marcar os diagramas depois de apagar uma página, com 999 correções | ≈ 1,7 s no thread da UI | ≤ 20 ms (G2) | G2b |
| PGN do livro no teto de lances | 9,4 s e 1,18 GB, no thread da UI | ≤ 300 MB, fora do thread da UI | I3b |
| Maior quadro do wasm durante "Detectar no livro" | o livro inteiro num quadro (minutos) | ≤ 100 ms (I2) | I2 |
| Exportação do livro inteiro com a varredura em cache: tempo · tamanho · pico | EPUB 8,4 s · 10,1 MB; HTML 5,4 s · 7,2 MB; DOCX 35,9 s · 118,5 MB · 667 MB | DOCX ≤ 25 MB; os outros como base | X1b |
| Focáveis do conteúdo de uma página da aba Texto (sem os controles fixos do painel) | 61 na pág. 13 (A6) | ≤ lances + diagramas | I9 |
| Texto abaixo de 4,5:1 / não-texto abaixo de 3:1 no painel de xadrez | lances cinza 2,61:1; caixa 1,39:1; barra 1,19:1 | 0 | I9 (texto), I11a e I8 (não-texto) |
| Nós focáveis sem nome | ≥ 15 no painel de xadrez (10 da aba Texto, 1 imagem, Origem do livro, Lance nº, Encaixar em, Idioma) + a caixa da página da barra (`chrome.rs:930`) | 0 | I15, I11a, I11b |
| Aba Diagrama: salto do tabuleiro quando aparece "Alterações não confirmadas" (o transbordo foi corrigido no #254) | 24 pt | 0 | I8 |
| Jornadas (A6): aceitar o PGN irmão do usuário · recusar a oferta · desfazer uma correção · ver um lance impresso no tabuleiro | 5–7 · impossível · 2 sem aviso · ≥ 3 | 1 · 1 · Ctrl+Z · 1 | I7a · I7a · G10 · N1 |
| Jornadas (R4): livro → EPUB salvo · diagrama → motor com lances visíveis · trecho → PGN salvo · corrigir no modo Ler | ≥ 7 + nome · 2 + rolagem · ≥ 5 + rolagem · impossível | 5 · 1 e 0 rolagens · 3 · igual ao modo normal | X2 · I0b · I0b + I16 · I15 |
| Largura da página com o xadrez aberto a 1366 · cromo / largura | ≈ 390 pt · 71 % | ≥ 940 pt · ≤ 35 % | I0a |
| Achar Tabuleiro, Analisar e Exportar livro por Ctrl+K | 0 de 3 | 3 de 3 | I16 |
| Alvos clicáveis do painel abaixo de 24 × 24 pt | ≥ 2 (Parar 22, Dispensar 20) | 0 | I15 |
| Controles do painel fora da vista a 1366 × 768 com o Tamanho da interface em 200 % | 3 (Exportar, Tabuleiro, Mais ações) | 0 | I0a |
| Descrições de diagrama exportadas com o lado errado (Harikrishna pp. 12–15) | 4 de 6 | 0 | L6b |
| Nós vivos (anúncios) no app · jornadas com um lance jogado só pelo teclado | 1 · impossível | todo aviso e toda navegação · possível (N7a) | I17 · N7a |
| Matriz WCAG 2.2 AA (§2.8.3): critérios que falham | 15 de 20 | 0, com o teste com TA real da D26 | I11b, D26 |
| Portão | 4 classes de quebra já escaparam | as 4 pegas por sabotagem; etapas 0–2 < 5 min | Q1a |
| Entradas que abortam o processo | SVG ≥ 4 formas (aninhamento de `<g>`, cadeias de `<use>`, `clip-path`, `mask`) · ONNX ≥ 1 (`ConstantOfShape` grande) | 0; recusa com motivo | V1, V8 |
| Página com meio-tom de 450 × 450 px | perde todo o OCR | lê as linhas | V2 |
| Inferências por página em itálico | 1.960 (6,96 s) | ≤ 10 (≈ 0,8 s) | V3 |
| Detectores no teto de entrada (200 mil glifos / 50 mil caixas), em release | 2,85 s / 1,58 s (17,2 / 3,0 s no perfil de teste) | < 1 s cada | V7 |
| Decisões espalhadas (contagem de sítios) | "mesma posição" ≥ 12 · `current_exe` abaixo de L7: 4 · tuplas `(usize, usize)` de diagrama em `chess_ui.rs`: 14 · rótulo de página: ≥ 9 · casadores de correção: 2 · stores por documento: 3 formatos | 1 regra/tipo cada (`grep` → 0 fora do dono) | T1a, T1b, T3, T2b, G2b, G3a |
| Re-percorrer = ler de novo (propriedade do walk) | falha (R3-01) | vale, com overlay | A9-L0a |
| Mutação nos módulos puros tocados | `rules.rs` 95,2 % | ≥ 90 %, sobreviventes explicados | Q4a |
| CC das funções que concentram bugs | `from_pgn_report` 60, `lex` 40, `play` 64, `apply` 74 | < 20 cada | F2b (`from_pgn_report`, `lex`), L9 (`play`), F6 (`apply`) |

## 6. Riscos

| Risco | Prob. | Impacto | Mitigação |
|---|---|---|---|
| Reescrever o leitor de PGN regride leituras que hoje funcionam | Média | Alto | Golden com o corpus atual + sondas; python-chess como oráculo; fuzz de `split_pgn`/leitor (Q3); dois PRs (passo: F2a, F2b, Q3b) |
| Migração dos stores (nome por tipo, local único) esconde correções | Média | Crítico | Mesclar, nunca mover nem copiar por cima; ler o nome antigo e mesclá-lo quando mudar (a mesma regra do `migrated.json`); contagens antes/depois registradas (passo: G3a, G5a) |
| Um build antigo (o `anterior-…` do usuário, uma CLI de outro worktree) regrava um store ou a cifra e apaga em silêncio o que o novo acrescentou | Média | Crítico | §2.10: formato sobe de versão quando a correção depende do campo novo (o antigo abre só para leitura); trava de nome novo; cifra grava o agregado antigo junto; teste "novo → antigo → novo" em cada passo que muda formato (passo: G2a, G3a, G4, G12, L1b; E0 (notas)) |
| Um modelo `.onnx` hostil ou mal exportado aborta o processo na carga | Baixa | Alto (trabalho não salvo) | V8 na onda 0 (op permitido e tamanho dos inicializadores antes de `Model::load`); teste com o modelo `bomb` |
| Os builds já instalados (≤ cc2d484c) põem de lado as coleções no formato novo | Média | Crítico | Formato novo como `<hash>.games.json`, nunca `*.games`; teste pelo código v1 congelado (passo: G12, G3b) |
| App, CLI, importação e duas abas gravam as mesmas partidas | Média | Alto | `uid`/`rev`/lápide, `merge` por tipo, dono por instância, autosave que mescla (§2.3) (passo: G3b) |
| Migração "única" com o build antigo ainda em uso | Alta | Alto | Migração que mescla e pode rodar de novo, com `migrated.json`; teste "corrigir no antigo depois" (G5) |
| Texto do livro e caminhos persistidos fora da política de documento protegido | Média | Médio | `persist_policy` única; pasta de dados só do dono; nenhum caminho em claro (passo: G3a, G3b) |
| Autosave em worker fora de ordem, ou perdido ao sair | Média | Alto | Um gravador por store, "último pedido vence", esperar ao sair (I3) |
| Cache N5 enche o disco ou serve uma entrada corrompida | Média | Médio | Teto com espaço livre, chave conferida na leitura, despejo tolerante (N5) |
| Mudar o `/ID` de documentos novos diverge do upstream | Baixa | Médio | Mudança mínima em `ensure_id`; nota em `docs/` e decisão D24 (passo: G1) |
| Regras do walker mudam leituras sem que o total mostre | Alta | Alto | Q2 antes de L4/L9: régua por lance e pela verdade do PGN do livro |
| `DocChess` toca ~40 usos em `ui-egui`, o arquivo mais disputado | Alta | Médio | I1 em dois PRs, depois de Q1; rebase no mesmo dia; uma sessão por vez em `chess_ui.rs` (passo: I1a) |
| Testes de UI instáveis mascaram falhas novas | Média | Médio | `known-failures.toml` que falha também quando um teste volta a passar (Q1); esperas por evento (Q4) |
| Builds lado a lado (`dist/app` e `anterior-…`) se entregam arquivos e dividem preferências e stores; nenhum diz qual build é | Alta | Médio | E2 (socket por pasta, id do build); E0 (anterior completo, notas da versão) |
| A ajuda em PDF e o guia descrevem a interface antiga | Alta | Médio | Passo 0 (fonte guardada); `PENDENTE.md` na Saída de cada passo; E1 |
| A análise e as provas se perdem na pasta temporária | Média | Alto | Passo 0 hoje |
| Disco C cheio durante builds | Média | Médio | `CARGO_INCREMENTAL=0`, `-j 2`, um build de release por vez, limpar `target/debug/incremental` (passo: Q1a, §0) |

## 7. Decisões do usuário

**Quando perguntar.** Já, numa mensagem só (respostas curtas que bloqueiam a onda 3): D17 (G5a), D25 (G4), D19 (F4), D28 (E0). Agendar nas ondas 1–2: a sessão da D18 (F7, X1b; o kit sai no F1, onda 0) e a instalação do NVDA da D26 (X3 na onda 3; I11b e N7a na onda 5). Na onda 6: D20 (N10), D21 (N5), D22 (N15), D23 (N18). Não bloqueiam: D24 (nota para o upstream), D27 (a detecção vai no L7; converter é outro passo). Nenhuma decisão bloqueia as ondas 0–2.

| Id | Decisão | Recomendação |
|---|---|---|
| D17 | Onde guardar correções, PGN do livro e partidas: na pasta de dados do app (não em "Recovery"), a mesma para o app e a CLI, com migração que mescla e pode rodar de novo (a origem fica) | Sim |
| D18 | Uma sessão única de 15 minutos no ChessBase 26 com um arquivo de teste preparado pelo agente, que responde: UTF-8 com e sem BOM e Windows-1252 (o ISO 8859-1 do padrão); que código o CB escreve para uma marca de diagrama; se o CB lê `$37`/`$23`; e o protocolo do `%tqu` da D20 | Fazer antes do F7 e do X1b; o F1 não espera: PGN exportado sem BOM por padrão, com a opção lembrada, até o resultado |
| D19 | "Copiar PGN" para a área de transferência também quebra em 79 colunas? | Sim (uma regra só) |
| D20 | Treino dos exercícios: o PGN de problemas padrão (todo programa lê) primeiro; o `%tqu` do CB só depois de testar, na sessão da D18: com ou sem campo de idioma, lance em UCI ou SAN, comando antes do 1º lance num exercício que começa na FEN, banco com o atributo "Training", mais de uma resposta certa | PGN padrão já; `%tqu` depois do teste |
| D21 | Leitura guardada em disco (N5): teto e lugar | 512 MiB na pasta de dados do app, livros menos usados saem primeiro |
| D22 | Biblioteca de livros (N15): construir? É a maior peça (L) | Depois do MVP e de N5 |
| D23 | Modo "resolver" no tabuleiro (reabre a decisão de 2026-10-25 de deixar treino fora) | Não agora; rever depois de D20 |
| D24 | Levar a correção do `/ID` (cos) ao PdfCraft original? Junto: o `/ID[1]` nunca muda numa regravação (a ISO 32000-2 §14.4 pede que mude a cada atualização); e o "Automático" do Tamanho da interface ignora o Tamanho do texto do Windows (`apps/pdfcraft/src/text_scale.rs:1-20` lê só o GNOME; R5-10) | Registrar em `docs/` e decidir na próxima sincronização |
| D25 | Ao carregar **outro** PGN do livro, manter as decisões de conflito já tomadas (revendo o R6e do `plano-pgn-do-livro.md`, que decidiu descartá-las)? | Manter e dizer quantas não casam mais |
| D26 | Quem testa com leitor de tela e lupa reais, e quando (a árvore AccessKit não diz o que o NVDA fala) | O usuário instala o NVDA portátil e roda, ao fim do I11, do N7a e do X3, um roteiro de 12 passos com o Visualizador de Fala ligado, salvando o registro no PR (um agente compara com o texto esperado); uma passada com a Lupa do Windows a 200 % e "seguir o foco do teclado". Antes de dizer "acessível" no `parity/`, uma sessão com um enxadrista cego (comunidade de xadrez para deficientes visuais ou usuários do modo cego do lichess) |
| D27 | Converter livros em notação descritiva (P-K4, P4R) para algébrica? É viável: o walk tem a posição, e a descritiva é relativa ao lado, então os candidatos se resolvem pela legalidade (M) | Detectar e avisar já (L7); converter se houver livros assim na biblioteca do usuário |
| D28 | Quando refazer o `dist/app` e a ajuda que você usa | Ao fim de cada onda (E0), guardando o anterior completo; a ajuda no fim (E1) |

## 8. Fora do escopo

Repetição espaçada dentro do app; motor embutido em Rust; "explique o lance" por regras; traduzir a prosa dos livros; EPUB/HTML com tabuleiro jogável (exigiria script); comparar edições; estatística de aberturas; correções compartilhadas pela internet; voz própria (o leitor de tela do sistema faz melhor). Razões em `analise-9/A8.md`.

## 9. Histórico de revisões

A v11 fecha as dez revisões (R1–R10). A tabela de todas as versões, com as notas de cada crítico, está no **Anexo B**.

## Anexo A — Achados

**C** crítico (perda de dados, pânico, resultado errado silencioso, segurança) · **I** importante · **P** polimento. "Passo" = onde o plano resolve; quando dois passos tocam um achado, o da coluna é o dono e o outro diz "parte de".

#### Guarda dos dados (stores, coleção, correções, confinamento)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A4-03 | C | Todo PDF criado pelo PdfCraft (extrair, dividir, combinar) sai com o **mesmo `/ID`**: livros diferentes passam a compartilhar correções, PGN do livro, coleção e identificador de EPUB. CONFIRMADO (`/ID <9BA0D5AA…>` em 5 arquivos). | `cos/src/writer.rs:358,562-577` (hash de bytes vazios com semente fixa); `engine/src/chess/keys.rs:35-40` | G1 |
| A4-01 | C | A correção de uma página apagada passa para o diagrama da página seguinte (mesma caixa), e o redesenho a desenha no PDF. CONFIRMADO. | `corrections.rs:456-466` (`locate`); o teste `corrections/tests.rs:133-136` consagra o caso | G2 |
| R1-01 | C | Pior que A4-01: apagar uma página corrigida faz a correção dela **vencer** a correção já confirmada do diagrama que sobe para o lugar (mesma caixa); a certa fica "sem diagrama" com um motivo falso. CONFIRMADO (CLI, `s_a401b.json`). | `corrections.rs:456-466` (`Changed` por índice) × `:650-683` (a 1ª no arquivo vence) | G2 |
| A4-02 | C | Recorrigir um diagrama que mudou de página cria duas correções vivas: as exportações usam a antiga, o redesenho a nova. CONFIRMADO. | `corrections.rs:394-398,445-451,672-684` | G2 |
| A4-11 | C | Exportações gravam por cima do PDF aberto ou do PGN de referência sem aviso: o PDF virou 759 bytes de Markdown; se a sessão fecha sem salvar (o documento não está sujo), a perda é definitiva. CONFIRMADO. | `automation/src/chess.rs:482-483,523-530,656-658,718` | G7 |
| A6-01 / A5-10 | C | Os resultados da detecção existem para um documento só: detectar no livro B apaga a lista do livro A, e o painel de A passa a dizer "Nenhum diagrama detectado ainda" (CONFIRMADO no app). Por leitura: o aviso de pendências da exportação de A não acha resultado (`filter(\|r\| r.doc == id)`) e é **pulado**; a cifra do livro e a origem também vivem numa vaga só (A5-10). C por resultado errado silencioso, não por perda: as correções ficam no store. | `chess_ui.rs:36-141,792-794,1008-1010`; `chess_export_ui.rs:126-128`; `chess_text_ui.rs:757-759,822-859,931-935` | I1 |
| A4-05 | C | `chess_games` com `out` = `path` corta no 1º lance impossível as partidas, inclusive as não editadas, e grava. CONFIRMADO. | `automation/src/chess_games.rs:115-122,139-150` | G7a (recusa), F3 (raiz) |
| A5-01 | C | Acima de 256 partidas a coleção avisa uma vez e para de gravar; Sair/Fechar não pergunta: o trabalho depois do aviso se perde. CONFIRMADO por leitura. | `chess_autosave.rs:272-274`; `chess_collection.rs:518-537,1259-1266`; `editing.rs:944-997` | G8 |
| A4-04 | C | `chess_pieces_import` sem `into` grava ao lado do executável, fora de `--automation-write-root`. CONFIRMADO via MCP. | `automation/src/chess.rs:882-888` | G7 |
| A6-04 | I | Uma correção confirmada não se desfaz com Ctrl+Z; "Esquecer a correção" apaga correção **e rascunho** sem perguntar nem avisar. CONFIRMADO no app. | `chess_corrections_ui.rs:356-366,388-414` | G10 |
| A4-06 | I | Uma entrada inválida põe de lado o arquivo inteiro de correções (16 → 1 visível), e as ferramentas de escrita não avisam. CONFIRMADO. | `corrections.rs:345-353,872-899`; `chess_corrections.rs:251-282` | G3 |
| A4-07 / A7-02 | I | Correções e PGN do livro na mesma pasta usam o mesmo nome de arquivo e travas diferentes: até uma **listagem** põe o outro de lado. CONFIRMADO. | `reference_store.rs:363,382`; `corrections.rs:503,838,866` | G3 |
| A4-13 | I | Ferramentas só de leitura exigem permissão de escrita e renomeiam arquivos (`.bad`). CONFIRMADO via MCP só-leitura. | `chess_corrections.rs:44`; `chess_reference.rs:65`; `corrections.rs:880-882` | G3 |
| A4-08 / A5-08 | I | Carregar de novo o (mesmo) PGN do livro apaga as decisões de conflito: não é idempotente. CONFIRMADO. | `reference_store.rs:314-321`; `chess_reference_ui.rs:427-445` | G4 |
| A4-12 | I | Dois lugares para os stores: CLI em `<root ou CWD>/.pdfcraft/…`, app em `%LOCALAPPDATA%\PdfCraft\Recovery\chess\…`; um não vê o que o outro gravou. CONFIRMADO. | `chess_corrections.rs:19`; `chess_reference.rs:21`; `ui-egui/recovery.rs:297-300` | G5 |
| A4-10 | I | `cipher_in` é lido inteiro antes do teto de 1 MiB (pico de 616 MB com 600 MB). CONFIRMADO. | `automation/src/chess.rs:771` | G7 |
| A4-09 | I | Links do lichess não convergem: depois de uma correção, um 2º rótulo e link são empilhados sobre o primeiro, que fica com a posição errada. CONFIRMADO. | `engine/src/chess/links.rs:234,279` | G9 |
| A5-07 | I | A lixeira de lances só é alimentada quando o painel Tabuleiro é desenhado e só quando a contagem cai: lances substituídos se perdem após reiniciar. CONFIRMADO por leitura. | `chess_tools.rs:957-963`; `chess_deleted.rs:76-90`; `chess_collection.rs:724-732` | G8 |
| A5-06 | I | Ctrl+Z na aba Diagrama sem passo no editor desfaz um lance da partida do tabuleiro (escopo misturado). CONFIRMADO por leitura. | `chess_diagram_keys.rs:11-13,119-127`; `editing.rs:196-200` | G10 |
| A5-09 | I | "Confirmar correção" que falha cancela Fechar/Sair em silêncio; rascunhos órfãos repetem a pergunta; "Continuar editando" não abre nada se a lista é de outro documento. CONFIRMADO por leitura. | `chess_drafts.rs:99-107`; `chess_corrections_ui.rs:216-220,290`; `lib.rs:1957-1976` | G10 |
| A4-14 | I | O cache de varredura identifica o modelo pela pasta: retreinar na mesma pasta deixa as leituras velhas. PLAUSÍVEL. | `engine/src/chess.rs:152-200,431-433` | G6 |
| A4-15 | I | Web: valor ilegível no `Storage` recomeça vazio e é sobrescrito; acima do orçamento some; abas concorrentes perdem escritas. PLAUSÍVEL. | `chess_corrections_ui.rs:127-151`; `corrections.rs:935-946` | I1 |
| R1-02 | I | O gêmeo de A4-05 na interface: Abrir PGN… + "Salvar todas como PGN…" grava cortadas, sem aviso no ato, as partidas que pararam num lance impossível (o motivo só aparece com a partida no tabuleiro). CONFIRMADO por leitura (o equivalente headless foi reproduzido). | `chess_collection.rs:2422-2431,2547-2569,2606-2620` | G8 |
| R1-03 | P | Ferramentas anunciadas `read_only: true` no esquema (`chess_diagram_states`, `chess_reference_status`) renomeiam arquivos do store: um cliente MCP que aprova sozinho as de leitura deixa mexer na pasta. CONFIRMADO. | `pdfcraft-cli tools` + roteiro `s4.json` de A4 | G3 |
| R7-03, R7-04, R7-05, R7-06 | P | Privacidade e saídas: a coleção grava o caminho do PDF em claro (15 de 20 `.games` com `C:\Users\<conta>\…`), enquanto correções e PGN gravam um hash (FNV sem sal, adivinhável); duas saídas da mesma chamada no mesmo caminho (`out` = `cipher_out`) — a resposta diz "exportado" e o arquivo tem o JSON da cifra; um build mais velho apaga as preferências que um mais novo gravou (`persist()` não preserva chaves desconhecidas); respostas MCP com caminhos absolutos fora da raiz (nome da conta). CONFIRMADO. | `chess_autosave.rs:83-103`; `automation/src/chess.rs:718,780-786,921,941-953`; `ui-egui/src/lib.rs:2308-2400` | G3b (R7-03), G7a (R7-04, R7-06), I18 (R7-05) |
| R2-03 | P | Página de correção vinda do arquivo sem validação: com `page = usize::MAX` gravado, a CLI release responde `recorded_page: 0` (soma com volta) e um build de teste entra em pânico. CONFIRMADO. | `corrections.rs:345-353`; `automation/src/chess_corrections.rs:115,309` | G3 |
| R2-05 | P | Web: os seletores de PGN leem o arquivo inteiro sem teto pelo `rfd::FileHandle::read` (sem `onerror`: uma leitura recusada fica pendurada para sempre; `unwrap` no `FileReader`); o seletor de modelos já faz certo. CONFIRMADO por leitura. | `chess_collection.rs:2516,2536`; `chess_reference_ui.rs:381`; × `chess_models.rs:328-340` | I1 |
| A4-19 | P | Um programa que não fala UCI continua rodando; o motor pode ficar órfão ao fechar o app. PLAUSÍVEL. | `analysis.rs:339,353-372`; `chess_study_ui.rs:292-305` | I13 |
| R3-04 | I | Partidas e lances apontam para a página **pelo índice** (`NodeSource.page`, `Source`, `StoredOrigin`, etiquetas `PdfPage`): inserir ou apagar uma página desloca "Mostrar no PDF/Seguir" e grava etiquetas de origem erradas — a classe do A4-01 no terceiro store. CONFIRMADO por leitura. | `chess/src/game/structure.rs:24-27`; `chess_board_ui.rs:166-171`; `chess_autosave.rs:153-166`; `chess_source.rs:168-184` | G2, T2 |
| R3-05 | I | O store de partidas põe de lado (`.bad`) arquivos de versão diferente **de qualquer documento** ao procurar um só — o oposto do `Read::Newer` das correções; o dia em que o formato mudar, o build antigo do usuário some com as coleções de todos os livros. CONFIRMADO por leitura. | `chess_autosave.rs:296-298,391,407-424,440-443` | G12 (política de versão), G3b |
| R3-06 | I | Três cópias vivas das correções (UI, sessão do motor, `ChessResults.kinds`) e dois casadores correção ↔ diagrama (`find` na UI, `apply` guloso no motor); o `CorrectionFile::merge` só roda em testes. CONFIRMADO por leitura. | `chess_corrections_ui.rs:162-166,179-180,224-240,428-438`; `corrections.rs:368,445-451,650-712` | G2 |
| R9-01 | I | O `StoredComments` do autosave é o que mantém certos os comentários nos builds instalados (o leitor deles perde o `comment_before` e corta acima de 4 KiB); apagá-lo enquanto o arquivo ainda é `*.games` v1 faz o build antigo regravar comentários fora do lugar ou cortados. CONFIRMADO por leitura. | `chess_autosave.rs:41,166-176,211-224,435` | I3a, G3b |
| A6-27 | P | Cópias portáteis compartilham o socket de instância única: o `dist/app` e o `anterior-…` que o usuário guarda podem trocar arquivos entre si. PLAUSÍVEL. | `apps/pdfcraft/src/single_instance.rs:155-163` | E2 |
| A4-16, A4-17, A4-20, A4-21, A4-22 | P | Trava "velha" com corrida e `Drop` que apaga a trava de outro; dois gravadores atômicos; `name_for` lê até 1.024 arquivos (até 64 MiB cada) por carga; trabalho guardado em "Recovery", sem cópia nem exportação, `MAX_ENTRIES` inalcançável; `rename` diz "mudou" quando a etiqueta foi cortada. | `analise-9/A4.md` | G3, G5, F3 |

#### PGN fiel (leitura e escrita)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A1-01 / A8-01 | C | PGN com BOM UTF-8: o 1º jogo perde a 1ª etiqueta (num arquivo de problemas, o FEN: o problema some) e aparece um jogo vazio a mais. CONFIRMADO. | `chess/src/game.rs:1514`; `game/split.rs:11`; leitores em `chess_collection.rs:2549`, `automation/src/chess.rs:1000`, `chess_games.rs:80`, `chess_reference_ui.rs:415` | F1 |
| A1-02 | C | A leitura corta e descarta em silêncio comentários > 4 KiB, etiquetas > 256 bytes, etiquetas além de 64, NAGs além de 8 ou do mesmo grupo; "Salvar PGN" grava a versão cortada. CONFIRMADO. | `game.rs:48,55,817-821,1020,1118,1246-1252,1457-1459,1547` | F3 |
| A1-03 | C | Uma variante que começa repetindo o lance da linha principal passa a ser a linha principal. CONFIRMADO (CLI). | `game.rs:630-633,1259-1261` | F2 |
| R6-01 | I | Comandos `[%clk]`, `[%eval]`, `[%emt]`, `[%tqu]` ficam no texto do comentário: toda partida baixada do lichess ou do chess.com aparece com cada lance "comentado" e "[%clk 0:02:58]" em itálico. CONFIRMADO (CLI). | `chess/src/game/drawing.rs:98-140`; `chess_board_ui.rs:1696-1705` | F7 |
| R6-04, R6-05, R6-06, R6-08, R6-10 | P | O `Result` lido é reescrito pelo tabuleiro (mate com `1/2-1/2` grava `0-1`; afogamento com `1-0` grava `1/2-1/2`) sem aviso; a etiqueta `Variant` é ignorada (Chess960 recusado como "posição ilegal", Crazyhouse lido como xadrez e cortado); o lichess descarta os NAGs das pretas que o app grava, e `$201` (Scid) não é marca de diagrama; `PlyCount`/`ECO` ficam velhos depois de editar; `Choice::Pgn` não guarda qual lance do PGN foi escolhido. CONFIRMADO (CLI). | `game.rs:1041-1095`; `fen.rs:158`; `nag.rs:172-182,263-276`; `reference_store.rs:52-68` | F5 (R6-04, R6-05) · F7 (R6-06) · F4 (R6-08) · G4 (R6-10) |
| A1-10 | I | PGN em Latin-1/Windows-1252 (o padrão §4.1 é ISO 8859-1) perde os acentos (`R�ti`) e os grava assim. CONFIRMADO. | os quatro leitores (`from_utf8_lossy`) | F1 |
| A1-05 / A7-01 | I | `split_pgn`: um comentário `;` ou uma linha `%` com `{` funde todos os jogos seguintes num só; eles somem sem contagem (`chess_reference_set` → `games: 1` de 3). CONFIRMADO. Três lexers decidem o que é comentário. | `game/split.rs:11-33`; `game.rs:1550-1583`; `doc/pgn.rs:345,394` | F2 |
| A1-06 / A2-02 | I | Comentário entre `)` e o lance seguinte (o "texto antes do lance" do ChessBase) vai para o lance anterior, antes das variantes — nos dois leitores e nos dois escritores. CONFIRMADO. | `game.rs:899-931,1225-1243`; `doc/pgn.rs:226-236,491-505`; `doc/games.rs:913-921` | F2, F4, L2 |
| A1-04 | I | `result()` transforma a regra dos 50 lances em empate automático e reescreve o `Result` lido (`1-0` → `1/2-1/2`). CONFIRMADO. | `game.rs:1041-1056,1411-1418` | F5 |
| A1-07 | I | Zugzwang lido de texto ganha o código do lado errado; a regra existe em três lugares com duas respostas; um teste fixa o erro. CONFIRMADO. | `nag.rs:352-376,475`; `graft.rs:444-449`; `chess_annotate.rs:393` | F5 |
| A1-08, A1-20 | I | FEN um pouco errada (`KQkq` fixo, lance 0, relógio > 100) recusa o jogo inteiro com "posição ilegal" sem motivo; números de lance até 65.535 repetem. CONFIRMADO. | `fen.rs:155-191`; `rules.rs:153-170`; `game.rs:1124-1127` | F3 |
| A1-09 | I | A leitura para no 1º símbolo comum não canônico (`e.p.`, `±`, `+-`, `…`, `e8Q`, `a8=q`). CONFIRMADO. | `game.rs:577-588,1469-1472,1504,1594` | F2 |
| A1-11 / A2-09 | I | Dois escritores com duas políticas: `Game::pgn` (coleção, PGN anotado do livro) grava cada partida numa linha só (até 13.501 caracteres); `doc::pgn` quebra em 79. O mesmo `chess_export_pgn` muda de formato conforme haja PGN do livro, e ignora `positions` com referência. CONFIRMADO. | `game.rs:1058-1095`; `doc/pgn.rs:24`; `engine/src/chess/export.rs:224-235` | F4 |
| A1-12 / R2-02 | I | O graft descarta um comentário novo que é substring do existente e relata sucesso; a mesma regra, copiada, faz a captura em modo comentário responder `applied: true` sem mudar nada e dizer "Nothing new: the moves are in the game already". CONFIRMADO (CLI). | `game/graft.rs:197-201,424-439`; `engine/src/chess/capture.rs:627-640` | F3 |
| A1-13 | I | Roque com "О" cirílico: o reparo o vê como roque, `San::read` não o lê. CONFIRMADO. | `san.rs:510-563`; `repair.rs:161-164` | F5 |
| A1-15 | I | Bater no teto de 10.000 lances é relatado como lance ilegal. CONFIRMADO. | `game.rs:639-641,1276-1279`; `chess_collection.rs:2614` | F2 |
| A2-18 | P | Dois leitores PGN; o round trip "exato" é provado com `doc::pgn::parse`, que nenhum caminho do usuário usa; limites em unidades diferentes (caracteres × bytes). CONFIRMADO. | `doc/pgn.rs:284-667`; `games.rs:109` × `game.rs:48` | F4 |
| A7-12 | P | `chess_pgn::apply` (CC 74) valida JSON e edita a partida no mesmo `match`; o esquema da ferramenta é outra cópia. | `automation/src/chess_pgn.rs:163-312` | F6 |
| R2-04 | P | `Square::new`/`Square::offset` entram em pânico (debug) com fileira ≥ 32: `then_some` avalia `rank * 8 + file` antes da condição. Latente (os chamadores passam fileiras < 8), mas é função pública de L0 que não é total. CONFIRMADO. | `chess/src/placement.rs:116` | Q4 |
| A1-16, A1-17, A1-18, A1-19, A1-21, A1-22, A1-23, A1-25 | P | Etiquetas na mesma linha/recuadas/`\` solta; `from_pgn_report` (CC 60) e `lex` (CC 40) são onde os bugs nascem; SAN representa lances impossíveis (`Nf4=Q`); `promote()` não diz quando recusou; `Result` fora do padrão some; regras de posição duplicadas em `rules.rs` e `recognize/decode.rs`; `next_variation` O(n·v) (4 ms por tecla com 2.000 lances); `Mate(0)` mostra 50 %. | `analise-9/A1.md` | F2, F5 |

#### Leitura do livro (texto → partidas)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| R3-01 | I | Re-percorrer uma página parte do walk anterior, não da leitura: o walk grava por cima de `m.san` e `m.review`, e `add_read_page` não os zera — re-percorrer com o diagrama corrigido acumula bandeiras falsas (`cxd4→exd4` e `cxd4→cxd4`) e mantém a SAN do contexto velho; as decisões do PGN do livro, chaveadas por `m.san`, deixam de casar. CONFIRMADO por teste. | `doc/build.rs:1049-1057`; `doc.rs:648-656`; `reference.rs:469-470,504-506` | L0 |
| R6-03 | I | O lado a jogar impresso ("Black to move", "Pretas jogam", "Weiß am Zug", "White to play and win") nunca é lido: o crate `chess` tem o tipo (`SideSource::Text`, `TextPageScope`), mas nada o produz; os 37 diagramas do Karpov e os 12 do Polgar lidos estão com o lado suposto. CONFIRMADO por leitura. | `chess/src/recognize/side.rs:24-44`; `engine/src/chess.rs:873`; `side_text.rs:1-17` | L6 |
| R6-02 | I | A marca de captura impressa é ignorada: "Qxf6" para casa vazia é jogado como Qf6 sem bandeira — o sinal mais barato de que o walk se desviou da posição do livro (5 de 1.070 válidos no Harikrishna, nenhum confirmado pelo PGN do livro). CONFIRMADO. | `chess/src/san.rs:439-463` | L5 |
| R6-07 | P | Livro em notação descritiva (P-K4, P4R) dá 0 partidas sem nota nenhuma. CONFIRMADO (PDFs sintéticos). | — | L7, D27 |
| R3-10 | P | A leitura de uma página depende do histórico da sessão e da própria página, pela cifra: `read_page` repara com `book.proven()` (que já inclui a evidência dela) antes de gravar as provas; a cifra nunca é guardada. PLAUSÍVEL. | `engine/src/chess/text_ocr.rs:253-255`; `cipher.rs:4-6,201-213` | V4, N5 |
| A2-01 | C | O texto impresso entre um diagrama e o lance que o resolve (ou antes do 1º lance de uma partida que começa no diagrama) nunca chega ao PGN, ao tabuleiro nem ao PGN anotado. CONFIRMADO: 4 livros reais, até 21 trechos e 3,3 mil caracteres num só. | `doc/games.rs:76-77,1832-1834,1859-1867` | L2 |
| A2-06 | I | Diagrama em fonte que o catálogo não reconhece pelo nome vira título-lixo e encerra a partida: a solução inteira fica sem posição; o Markdown ganha `## 4\*+\*+p+…`. CONFIRMADO com o `OTS-derived-font` do Chrome — esse caso o #254 já corrige (o nome vem do programa da fonte); continua para uma fonte de diagrama de família desconhecida (fonte própria da editora, nome trocado também na tabela `name`), e a causa por geometria (L1) continua. | `chess_text/glyph.rs:43-55`; `doc/build.rs:752`; `games.rs:1320-1336` | L1 |
| A2-04 | I | Número de página e cabeçalho corrente são decididos só para o walker e vazam em HTML, EPUB (sumário com "Issue 47" ×10), Word, Markdown e no PGN anotado (`{32}`). CONFIRMADO. | `doc/build.rs:869-943,971-972`; `html.rs:437-449,515-527`; `markdown.rs:133-145`; `office.rs:164-167`; `annotate.rs:157-196` | L1 |
| A2-03 | I | Lances citados em prosa entram na partida ("…does not come to d3"): 7 jogados como válidos e 34 ilegais (com cascata) em 60 pp.; e o `walk_bench`, que decidiu a regra, conta lance errado como acerto. CONFIRMADO contra o PGN do livro. | `doc/games.rs:897-903`; `examples/walk_bench.rs` | L4 |
| A2-05 | I | Parágrafos: a dica de layout vence a geometria — 105 de 619 parágrafos partidos no meio da frase (Harikrishna), 203 de 993 (*Chess Evolution*); e o inverso, cinco parágrafos colados num. CONFIRMADO. | `doc/build.rs:776-783` | L3 |
| A2-08 | I | O idioma das exportações é o das letras das peças, não o do livro: livro brasileiro com figurinas sai com `lang="en"`, "White to move" e Word com ortografia inglesa. CONFIRMADO. | `engine/src/chess/document.rs:439`; `export.rs:180,207,265,281` | L6 |
| A2-07 | I | Depois da 4.096ª partida o resto do livro fica sem partidas, sem aviso (o Polgar perderia ~1.200 soluções). CONFIRMADO. | `doc/games.rs:1456-1458` | L7 |
| A2-10 | I | O tokenizador corrige em silêncio (`6.Rel` → `Re1`) também em PDF nascido digital, sem `ReviewFlag`. CONFIRMADO. | `chess_text/token.rs:225-442` | L5 |
| A2-11 | I | Depois de `reference::link` confirmar diagramas, as partidas não são refeitas: re-percorrer muda 23 lances em 60 pp. — 18 viram válidos (17 eram NotReached) e 5 pioram. CONFIRMADO. | `doc/reference.rs:541-562` | L8 |
| A1-14 | I | A descrição acessível diz "brancas jogam" quando o lado foi só suposto (HTML, EPUB, Markdown, Word). CONFIRMADO por leitura. | `fen.rs:135-140`; `describe.rs:115-125`; `html.rs:354-357` | L6 |
| A7-10 | P | `Walker::play` (CC 64, 258 linhas) é uma cadeia de regras heurísticas numa função só; as regras têm chaves (`WalkRules`), não fronteiras. (O A7 o deu como I; aqui fica P porque é refatoração sem mudança de comportamento — sobe para I se L4 ou L2 travarem nela.) | `doc/games.rs:1594-1851` | L9 |
| A2-12, A2-15, A2-16, A1-24 | P | Rótulos `A)`/`B2.1`, colchetes e `=====` lidos como estrutura; hifenização desfaz compostos ("wellknown"); `pages` fora de ordem/repetido quebra partidas e repete ids no HTML; nomes de cidade (`zurich`, `berlin`) tomam fontes de texto por fontes de xadrez. | `analise-9/A2.md`, `A1.md` | L7 (A2-12, A1-24), L3 (A2-15), G7 (A2-16) |

#### Visão (detecção e leitura de diagramas, glifos, peças, fontes, modelos)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A3-01 | C | Um SVG de peça com ~600 grupos `<g>` aninhados (13 KB) estoura a pilha e **derruba o processo** — importar peças, abrir a prévia das Preferências ou carregar uma página salva; nenhum `catch_unwind` segura um estouro de pilha. CONFIRMADO na CLI release (`thread 'main' has overflowed its stack`). | `pieces/svg.rs:61-63,112-115` (limita a contagem de nós, não a profundidade) | V1 |
| R2-01 | C | SVG de peça **plano** também aborta: cadeias de referências que o `usvg` resolve recursivamente — 300 `<use>` encadeados (10,5 KB, profundidade XML 2), 3.000 `clip-path` ou 3.000 `mask` (277 KB) — derrubam a CLI release; um limite de profundidade XML não pega. CONFIRMADO. | `pieces/svg.rs:51-64,112-115` (`from_xmltree`) | V1 |
| R7-01 | C | Um modelo ONNX **válido** de 1,29 MB (`ConstantOfShape [65536,65536,64]` → ReduceSum) derruba o processo já na carga — só `chess_status` com a pasta de modelos: exit 127, "memory allocation of 1099511627776 bytes failed"; o `rten` dobra constantes na carga, e a mutação de bytes do A3 não podia gerar esse grafo. Na interface, escolher a pasta de modelos aborta na hora. CONFIRMADO (CLI). | `chessvision/src/models.rs:160-164` | V8 |
| R7-02 | I | Substituir um conjunto de peças apaga o antigo antes de gravar o novo: com um arquivo aberto por outro processo (antivírus, OneDrive), `chess_pieces_import {replace: true}` falha no meio e sobram 6 de 12 peças — o conjunto em uso fica inutilizável e o novo não é gravado. CONFIRMADO (CLI). | `chessvision/src/pieces/import.rs:171,287-307` | V6 |
| A3-02 | I | Uma área de pontilhado (meio-tom de 450 × 450 px, poeira) passa do teto de 20.000 componentes, que conta até pontos de 1 px: `frames`/`read` falham e **a página inteira** perde o OCR de xadrez, sem aviso; `text_scale` usa outro teto (500.000) para a mesma pergunta. CONFIRMADO. | `glyphs.rs:63,65,264,294`; `seg.rs:287-303`; `engine/src/chess/text_ocr.rs:232` | V2 |
| A3-03 | I | O leitor de glifos faz uma inferência por par candidato a fusão e uma por linha: uma página em itálico custa 1.960 chamadas e 6,96 s contra 0,84 s (≈ 3 ms fixos por chamada). CONFIRMADO com o modelo real. | `glyphs/classify.rs:210-230,262,299`; `glyphs.rs:307-308` | V3 |
| A3-04 | I | A cifra do livro não é idempotente: reler uma página soma a mesma evidência de novo; 3 provas gravadas duas vezes viram "comprovado" (o mínimo medido é 5) e passam a reescrever lances no livro todo. CONFIRMADO. | `cipher.rs:33,138-144,201-207`; `chess_text_ui.rs:758,822-859` | V4 |
| A3-05 | I | Qualquer arquivo que comece com `wOFF`/`wOF2` (até 4 bytes de lixo) é aceito como fonte de figurinas e embutido no HTML/EPUB **sem checar `fsType`**; um `.woff` em ordem alfabética toma a vaga de um TTF verdadeiro. CONFIRMADO. | `text_font.rs:237-243,303-329` | V5 |
| A3-07 | I | Importar peças PNG e até abrir a lista de peças traça as imagens na thread da interface, duas vezes, sem reduzir as grandes: 12 PNGs de 2048 px = 2,4 s; um de ruído = 4,2 s (um conjunto ≈ 50 s); o comentário de `TARGET_GRID` promete o contrário. CONFIRMADO. | `pieces/images.rs:28-29,196`; `pieces/import.rs:275`; `chess_pieces_ui.rs:76-77,262-263` | V6 |
| A3-08 | I | Dois detectores quadráticos: 200 mil glifos de fonte de diagrama na mesma linha = 17,2 s por página; uma tabela de 50 mil quadrados = 3,0 s; o cancelamento só é checado entre páginas. CONFIRMADO. | `detect/font.rs:224-252`; `detect/vector.rs:91-104` | V7 |
| A3-06, A3-09, A3-10, A3-11 | P | O bit "só bitmaps" do `fsType` (0x0200) é ignorado; o OCR de página binariza e rotula a mesma imagem 4–5 vezes (115–170 ms por página, cinco cópias de 8 MP); um modelo com 1 bit trocado nos pesos carrega e erra a classe em silêncio (8 de 400), e o `*.parity.json` distribuído nunca é usado; dois Douglas–Peucker, o das peças recursivo e sem limite. CONFIRMADO. | `analise-9/A3.md` | V5, V2, V8, V9 |

#### Forma do dado (tipos que servem a todas as ondas)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| R3-03 | I | "Mesma posição" é decidida por fatias de FEN em ≥ 12 lugares, com três equivalências (só peças; peças + lado; quatro campos); a FEN é `String` em todas as camadas (≈ 80 chamadas a `fen::parse` fora de testes, 21 na UI). CONFIRMADO por leitura. | `games.rs:1018-1019`; `markdown.rs:365-366`; `reference.rs:226,257,295-303,544,613,701`; `doc/pgn.rs:178`; `chess_diagram_tab.rs:49`; `chess_ui.rs:1438`; `chess_board_ui.rs:212` | T1 |
| R3-09 | P | O mesmo conceito com dois tipos: endereço de lance (`MoveAt` × `InlineAt` × campos soltos de `Decision`), página (`engine::chess::ChessPage` × `chessvision::doc::ChessPage`), partida (`pdfcraft_chess::Game` × `doc::games::{Game, GameScore}`), identidade do documento (`DocKeys` × `DocRecord` × `StoredKeys`), lance em letras (`San::write` × `in_letters`). CONFIRMADO por leitura. | `span.rs:38`; `reference.rs:332`; `reference_store.rs:62-68`; `games.rs:194,629` | T2, G1, I10 |
| R3-08 | P | O rótulo de página é decidido em ≥ 9 lugares com três regras; o nome da partida, o editor clássico e a aba Texto mostram o índice + 1, a lista e a aba Diagrama o número impresso — "Página 13" e "25" para a mesma página. CONFIRMADO por leitura. | `html.rs:85-91`; `markdown.rs:125-130`; `office.rs:161-162`; `chess_collection.rs:147`; `chess_editor.rs:634`; `chess_text_ui.rs:409` | T2 |
| R3-07 | P | "Onde fica o app" decidido em cinco lugares, quatro abaixo de L7 (`current_exe()` em modelos, peças, fontes, motor; a pasta portátil à parte): A4-04, A6-10, A6-27 e D17 são sintomas da mesma decisão espalhada. CONFIRMADO por leitura. | `chessvision/src/models.rs:320`; `pieces.rs:390`; `text_font.rs:333`; `engine/src/chess/analysis.rs:63`; `ui-egui/src/portable.rs:52` | T3 |
| R3-02 | P | A orientação do diagrama só existe como sufixo da URL do lichess; um diagrama confirmado pelo PGN perde `?color=black`; a regra da URL está em 8 lugares com 3 orientações (os links gravados no PDF são sempre do lado das brancas). CONFIRMADO por teste. | `doc/reference.rs:556-558`; `board/reading.rs:178-186`; `engine/src/chess.rs:455-466`; `links.rs:78-84` | L6, G9 |

#### Desempenho e escala (medidos na CLI release, Ryzen 5 8400F 6c/12t)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| R8-01 | I | Os jobs da aba Texto, da exportação e de "Ligar o livro inteiro" abrem uma `Session` nova: o que "Detectar no livro" já leu é ignorado, e o que eles leem é jogado fora. Cada exportação relê as 748 páginas: 304 s frio contra 5,2 s com a varredura em cache (58×). CONFIRMADO (caminho inteiro + medida na mesma sessão MCP). | `chess_text_ui.rs:768-799`; `engine/src/chess.rs:1030-1061`; `chess_ui.rs:1001-1007` | G11 |
| R8-02 | I | Detectar e ler o livro usa **um núcleo** de seis (detectar 100 pp.: 24,1 s de parede, 23,6 s de CPU); o livro inteiro leva 4,7 min na CLI (≈ 7 min no app) e ≈ 12 min no *Chess Evolution*. CONFIRMADO. | `chess_ui.rs:797-815`; `engine/src/chess.rs:522-555` | V10 |
| R8-03 | I | Na web, toda tarefa longa de xadrez roda **dentro de um quadro**: a aba congela até o fim do livro e Parar não pode ser clicado; o parity diz "in the background". CONFIRMADO por leitura. | `chess_ui.rs:824-825`; `chess_text_ui.rs:797-798` | I2 |
| R8-04 | I | Marcar os diagramas com as correções custa O(D·C), e O(D·C·P) depois de inserir/apagar uma página: 1,7 s no thread da UI (748 pp., 999 correções) e ≈ 73 s na escala do Polgar (réplica em release). PLAUSÍVEL no app. | `chess_corrections_ui.rs:224-243`; `corrections.rs:439-466` | G2 |
| R8-05 | I | Toda edição depois de detectar refaz, no motor, o hash de conteúdo das 748 páginas (≈ 33 ms mesmo mudando só o Título) e toda leitura com correções refaz as impressões do livro (+70 ms): o I3 como escrito não chegaria a 16 ms. CONFIRMADO (MCP). | `engine/src/chess.rs:301-303,1106-1107,1153`; `page_key.rs:1-15` | G2, I3 |
| R8-06 | I | PGN do livro no teto (999.847 lances): 9,4 s e **1,18 GB** de pico para indexar, refeito a cada chamada da CLI e, na UI, no thread da interface (três `String` por posição, as partidas inteiras guardadas). CONFIRMADO (MCP). | `chessvision/src/doc/reference.rs:53-59,117-150`; `chess_reference_ui.rs:331,416` | I3, T1 |
| R8-07, R8-08, R8-09 | P | A lista de lances em "Colunas" monta todas as linhas a cada quadro e navegar regera o PGN inteiro a cada tecla (2,1 ms com 9.922 lances); `chess_pgn_edit` clona a partida por `play` e `next_variation` refaz os tokens (2.000 operações = 2,3–2,7 s); o DOCX do livro inteiro tem **118,5 MB** (cada diagrama duas vezes: PNG RGBA de 900 px e SVG de 109 KB sem reaproveitar as peças), 35,9 s e 667 MB de pico. CONFIRMADO. | `chess_board_ui.rs:514-528,1922-1963`; `automation/src/chess_pgn.rs:176-187`; `engine/src/chess/office.rs:69,192`; `export/src/flow.rs:13,168` | I0, F5, F6, X1 |

#### Exportações

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A2-13, A2-14, A2-17 | P | Markdown: `---` vira linha horizontal e `&copy;` vira ©; pasta de imagens com `#`/`%` quebra os links; U+FFFE/U+FFFF passam pelo escape XHTML/SVG (EPUB mal formado). CONFIRMADO. | `markdown.rs:62-65,87-105,158-165`; `html.rs:94-109`; `chess/src/svg.rs:212-228` | X1 |
| A4-18 | P | ZIP (EPUB/DOCX) trunca `as u32`/`u16` e engole erro do deflate. PLAUSÍVEL. | `export/src/zip.rs:39-78` | X1 |

#### Interface — código

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A5-02 | I | O autosave recodifica **todas** as partidas no thread da interface (PGN + releitura + nós): ≈ 300 ms a cada 2 s depois de cada lance, com as 120 partidas do livro. CONFIRMADO (medido). | `chess_autosave.rs:201-227,275`; `chess_collection.rs:1200-1219,2111-2126` | I3 |
| A5-03 | I | O job da aba Texto não tem guarda: um pânico no worker deixa `chess.job` preso para sempre (os outros três jobs têm). CONFIRMADO por leitura. | `chess_text_ui.rs:776-810` × `chess_ui.rs:390-398` | I2 |
| A5-12 | I | Uma vaga de job para tudo e sem Parar: "Ligar o livro inteiro" (minutos) bloqueia em silêncio a leitura de página da aba Texto. CONFIRMADO por leitura. | `chess_ui.rs:135,676`; `chess_text_ui.rs:72-77,675-677` | I2 |
| A5-11 | I | Passadas no livro inteiro no thread da interface: ≈ 90 ms de impressões digitais (748 pp.) a cada edição do PDF depois de uma varredura; montagem dos 94 jobs de "Detectar no livro" também. CONFIRMADO o custo. | `chess_corrections_ui.rs:200-206`; `chess_ui.rs:788,864-867,918` | I3 |
| A5-04 | I | "Diagrama N" na aba Texto varre a página **da vista** (não a do texto) e seleciona antes de o resultado chegar: o 1º clique não faz nada no app real; os testes passam porque a varredura é síncrona neles. CONFIRMADO por leitura. | `chess_text_ui.rs:918-925`; `chess_ui.rs:765-766,817-823,1053-1054` | I4 |
| A5-05 | I | "Abrir no lichess" da lista abre a leitura, não a correção confirmada; "Abrir no tabuleiro" some para diagrama não lido mas corrigido. CONFIRMADO por leitura. | `chess_list.rs:439-440,480-507` | I4 |
| A5-13 / A6-08 | I | Mensagens do motor em inglês aparecem cruas na interface em pt-BR ("…desta página (the page has no text layer): leia-a…"). CONFIRMADO no app. | `engine/src/chess/document.rs:350-369`; `chess_text_ui.rs:414-425,826,896-898`; `chess_ui.rs:449` | I5 |
| A5-14, A5-15, A5-16, A5-17, A5-18, A5-19 / A7-19 | P | Dois "stores por documento" copiados na UI (já divergem); quatro implementações de trabalho em segundo plano; "mesma partida intocada" decidida em três lugares (13–18 ms por clique); "Revisar agora" ignora o chip; PGN do livro indexado no thread da UI; pequenas duplicações (`square_at` em 4 lugares, rótulo de status, desfazer do editor fora de `History<S>`, `legal_moves`, FNV). | `analise-9/A5.md`, `A7.md` | I1 (A5-14), I2 (A5-15), G8 (A5-16), I4 (A5-17), I3 (A5-18), I6 (A5-19, A7-19) |

#### Interface — app rodando (visual, interação, textos, acessibilidade)

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A6-02 | I | A oferta do PGN irmão com nome longo alarga o painel a 1360 px: "Agora não", Exportar, Tabuleiro e ⋯ saem da tela; recusar com o mouse é impossível. CONFIRMADO. | `chess_ui.rs:608-617`; `chess_text_ui.rs:203-209`; captura `09` | I7 |
| A6-03 | I | O PGN ao lado do PDF não é oferecido para o par real do usuário: o PDF começa com "📚" e a comparação exige o mesmo nome-base. CONFIRMADO. | `chess_reference_ui.rs:595-599` | I7 |
| A6-05 | I | Aba Diagrama: o aviso "Alterações não confirmadas" faz o tabuleiro pular 24 pt. CONFIRMADO. (O transbordo a 1360 e a 1440 px foi corrigido no #254: a causa era `ui.available_width()` dentro do `ScrollArea`, que não desconta as margens do corpo; o tabuleiro passou a sair de `body_rect` por `board_room`/`board_side`, com o teste `the_diagram_tabs_board_stays_inside_the_tab`.) | `chess_diagram_tab.rs:172,252-266`; capturas `14`, `15` | I8 |
| A6-06 | I | Aba Texto: lances "não conferidos" em cinza fixo #a0a0a0 (2,61:1, abaixo de 4,5:1) — quase todos os lances da página; o estado é dito só pela cor (válido, ilegal, fora de sequência). CONFIRMADO. | `chess_text_ui.rs:313-320,533` | I9 |
| A6-07 | I | Aba Texto para teclado e leitor de tela: 74 focáveis no painel com a pág. 13 na aba Texto — 50 `Label` (prosa, colchetes, sinais), 10 nós sem nome, 1 imagem sem nome e 13 outros controles; o lance branco "6.Bb5+" é lido "6.♝b5+" (bispo **preto**). CONFIRMADO por `inspect`. | `chess_text_ui.rs:533,583,594` | I9 |
| A6-09 | I | A preferência de notação (figurinhas / letras no idioma / inglês) vale só para a lista; trilha, marcador de comentário, título da captura e linhas do motor usam SAN inglês ("6…Kf8"). CONFIRMADO. | `chess_tools.rs:1335-1347`; `chess_study_ui.rs:665-670` | I10 |
| A6-10 | I | O assistente "Nenhum motor instalado" não é um diálogo para a tecnologia assistiva (sem papel, sem foco, os 131 botões do fundo continuam na árvore); (o motor em `engine/stockfish/` passou a ser achado no #254, até três níveis). CONFIRMADO. | `chess_study_ui.rs:684-690`; `widgets::show_modal` | I11, I13 |
| A6-11 | I | Controles sem nome acessível (caixa da página, Origem do livro, Lance nº, Encaixar em, Idioma); abas Diagramas/Texto com papel de botão; "Agora não" com nome acessível que não contém o rótulo visível (WCAG 2.5.3). CONFIRMADO por `inspect`. | `chess_ui.rs:585,613-615`; `widgets::dock_tabs` | I11 |
| A6-12 | I | A 1366 × 768 a análise fica espremida: lista de lances numa janela rolável de 120 pt dentro de um painel rolável; ajustes de captura antes do tabuleiro; na janela destacada a lista começa fora da janela. CONFIRMADO. | `chess_board_ui.rs`, `chess_window.rs`; capturas `24`, `54` | I0 (I0b, I0c) |
| A6-13 | I | Linha da lista: o estado passa por baixo do ⋯, o título vira "Diagra…" e aparecem estados contraditórios ("Confirmado pelo PGN · Alterações não confirmadas"). CONFIRMADO. | `chess_list.rs:454-456` | I8 |
| A6-14 | I | A aba Diagrama não mostra o recorte do diagrama original; ao abri-la a página cai para 29 % e o diagrama do livro fica com ~95 px: conferir casa por casa exige zoom e rolagem. CONFIRMADO. | `chess_diagram_tab.rs:1-19` | I0a (Original/Lido) · I13 (`chess_diagram_crop`) |
| A6-15, A6-17, A6-18, A6-20 | P | Conteúdo cortado sob a barra de rolagem flutuante (linhas do motor, parágrafos, combos); contraste não-texto < 3:1 (borda da caixa 1,39:1, barra de progresso 1,19:1, peão do placar no escuro 1,39:1) e "×" como Parar; menus acionados por tecnologia assistiva não fecham; anel de foco não visto (PLAUSÍVEL). | `analise-9/A6.md` | I8 (A6-15, A6-17) · I15 (A6-18: menus que fecham) · I11a (A6-18: papéis; A6-20) |
| A6-16, A6-21, A6-22, A6-26 | P | Chips "Todas 999", "Lido 54"; "Procurar" × "Detectar diagramas" para a mesma ação, "sinal" × "glifo", selo de desenvolvimento "Pronto" e id interno no tooltip; Preferências com conjuntos em inglês, temas truncados, rótulos sem verbo, sem Fechar e com 1.450 pt de rolagem; "Lendo a página…" duas vezes, "12691" sem separador, aviso cobrindo o rodapé. | `analise-9/A6.md` | I15 (A6-16: chips) · I14 (resto) · I17 (A6-26: aviso sobre o rodapé) |
| A6-23 | P | Aba Texto com entrelinha irregular dentro do parágrafo (um `Label` por inline). CONFIRMADO. | `chess_text_ui.rs:485-490` | I9 |
| A6-24 | P | No modo Navegar o tabuleiro mostra "Lance nº: 1" editável na posição do lance 7 e "Quem joga" desabilitado. CONFIRMADO. | `chess_board_ui.rs` | I0 (I0b) |
| A6-25 / A8-18 | P | O tabuleiro de análise é uma imagem "Tabuleiro: <FEN>" para a tecnologia assistiva; o da aba Diagrama já é uma grade de 64 casas nomeadas. CONFIRMADO. | `chess_board_ui.rs:1038-1039` | N7b (o nome por `describe` já no I15) |
| R4-03 | I | A 1366 × 768 o xadrez ocupa 71 % da largura (Ferramentas 272 + Diagrama 320 + Inspetor/Tabuleiro 330 + trilho 48): a página cai para 29 % e dois tabuleiros aparecem lado a lado; avisos empilhados (200 pt) antes da ação principal. A causa sistêmica do A6-14 e do A6-12: superfícies somadas em colunas, sem orçamento de largura. CONFIRMADO no app. | `docking.rs:74-121,293-306,628`; `chess_ui.rs:626-652`; capturas `09`, `14`, `24`, `r04` | I0 |
| R4-01 | I | No modo Ler, clicar num diagrama não mostra o editor (não há como corrigir); ao sair do modo, a aba Diagrama surge e a página cai de 103 % para 29 %. CONFIRMADO no app. | `chess_ui.rs:1097-1104`; `docking.rs:451-463`; `lib.rs:3444-3486` | I15, I0 |
| R4-02 | I | O xadrez não está nos menus nem na paleta: 47 comandos `chess.*` sem menu, nenhum para abrir o Tabuleiro, Analisar ou Exportar o livro; Ferramentas › Xadrez com 2 itens truncados; o ícone do trilho só aparece depois de detectar. CONFIRMADO por leitura. | `engine/src/commands.rs:145-151,378-401`; `catalog.rs:289`; `chrome.rs:666,693-694` | I16 |
| R4-04 | I | "Exportar…" quer dizer duas coisas (livro × "Salvar todas como PGN"), e a exportação do livro não tem diálogo: opções em submenus de rádio que fecham a cada clique; três PGNs diferentes saem do app sem dizer qual leva o quê. CONFIRMADO. | `chess_panel_head.rs:58-63`; `chess_collection.rs:1725`; `chess_export_ui.rs:53,61-89` | X2, I15 |
| R4-05 … R4-13 | P | Cor de estado de diagrama usada para o lado a jogar; alvos abaixo de 24 pt e seis tamanhos de botão-ícone; ícones com dois sentidos; rodapé "Só confirmar" sem fila; erros com estilo de dica; retorno das ações em três componentes (o toast cobre os rodapés); textos ambíguos ("Ligar", "Encaixar"); com o xadrez numa janela própria o editor abre na principal (PLAUSÍVEL); a tela grande não é usada. | `analise-9/R4.md` | I15, I8, I17, I14, I0 |
| R5-01 | I | O escopo de teclas do xadrez não segue o foco e não sai pelo teclado (só um clique na página o desfaz): enquanto dura, setas, Home/End, PgUp/PgDn, Delete, as letras B C F G O X e os sinais `!?=+-/~0` são tirados do controle focado — o divisor de painéis deixa de responder às setas, ←/→ deixam de virar a página; atalhos de um caractere sem como desligar (WCAG 2.1.1, 2.1.4). CONFIRMADO por leitura. | `chess_tools.rs:689-733,1262-1281`; `chess_history.rs:347-361`; `canvas.rs:2423` | I18 |
| R5-02 | I | Navegar pela partida e terminar uma detecção de 7 min não dizem nada ao leitor de tela; os 99 avisos `notify*` (34 de erro) somem em 3,5 s sem papel de status; o único nó vivo do app é o da aba Diagrama (WCAG 4.1.3). CONFIRMADO por leitura e `inspect`. | `chess_board_ui.rs:1735-1737`; `widgets.rs:522-558`; `chess_ui.rs:899`; `chess_diagram_tab.rs:222-229` | I17, I15 |
| R5-03 | I | Não há como jogar um lance nem desenhar setas pelo teclado no tabuleiro de análise (a função principal do estudo; WCAG 2.1.1, nível A); setas e círculos só por arrasto ou botão direito (2.5.7). CONFIRMADO por leitura. | `chess_board_ui.rs:1034-1071` | N7a |
| R5-04 | I | Com o Tamanho da interface em 200 % (680 × 352 pt lógicos), Exportar, Tabuleiro e Mais ações saem do painel, títulos ficam sob o ×, a barra perde o campo da página e o zoom (WCAG 1.4.4, 1.4.10); o orçamento de §2.8.0 supunha 1 pt = 1 px. CONFIRMADO no app. | `docking.rs:74,81,628`; captura `R5/shots/r5-02-200pct.png` | I0 |
| R5-05 | I | A descrição dos diagramas exportados diz o lado errado em 4 de 6 casos (Harikrishna pp. 12–15: "White to move" com "Position after: 5.dxc5" logo abaixo), no EPUB, HTML, DOCX e no link do lichess; o walk já sabe o lado pela legenda e pelo número do lance. CONFIRMADO pela CLI. | `doc/games.rs:1036-1044,1181-1208`; `html.rs:354-357` | L6 |
| R5-12 | I | Lances de legenda (`Caption`) em azul fixo `#3B82C4`: 4,06:1 no claro e 3,72:1 no escuro, abaixo de 4,5:1. CONFIRMADO. | `chess_text_ui.rs:319` | I9 |
| R5-06 … R5-11 | P | Exportações que não servem a quem não vê (sinais só com `title`, modo FEN sem descrição, marca de revisão só por cor 1,11:1, figurinas sólidas "black knight" num lance branco, `accessibilitySummary` constante que afirma o falso, `<title>` igual em toda página); grade do editor com a FEN no nome e células sem `bounds`; menus sem papel de menu e `ui inspect` sem `live`/`modal`/`bounds`; prévia de lance que não fecha com Esc (1.4.13); o "Automático" do Tamanho da interface ignora o Tamanho do texto do Windows; sinais digitados com espera de 0,6 s. | `analise-9/R5.md` | X3 (R5-06) · I15 (R5-07) · Q5 + I11a (R5-08) · I10 (R5-09) · D24 (R5-10) · I18 (R5-11) |
| A6-19 | P | O canal de controle não alcança o teclado do xadrez (teclas injetadas chegam depois do `raw_input_hook`): as jornadas de teclado não têm teste ponta a ponta. CONFIRMADO. | `lib.rs:3172-3178`; `control.rs:197-201` | Q5 |

#### Qualidade e portões

| Id | Sev | Achado | Evidência | Passo |
|---|---|---|---|---|
| A7-03 | I | Não há portão local único; os portões por crate já deixaram passar quatro classes de quebra (campo novo quebrando a CLI #185; guarda `file_access` vermelho de R2 a R5; TOML do parity quebrado de #104 a #109; arquivos CRLF três vezes). | `xtask/src/gates.rs:97-102`; memória do projeto | Q1 |
| A7-04 | I | Os testes com os modelos reais e com motor UCI real se pulam em silêncio sem a variável, embora os modelos estejam em `dist/app/models`: o caminho principal do app distribuído não tem regressão. | `models.rs:567`; `glyphs/tests.rs:981`; `engine/src/chess/tests.rs:595`; `text_ocr.rs:242,436`; `automation/tests/chess.rs:2114`; `ui-egui/tests/chess.rs:2057` | Q1 |
| A7-05 | I | "Falhas conhecidas" vivem na memória, não numa lista: uma falha nova se esconde entre elas. | `chess-editing-plan` (memória) | Q1 |
| A7-06 | I | As varreduras de código-fonte (guarda `file_access`, cobertura de i18n) estão presas nos binários de teste mais caros; reimplementadas em texto rodam em 0,2 s. CONFIRMADO. | `automation/src/file_access.rs:343-386`; `ui-egui/src/i18n/mod.rs:929-960` | Q1 |
| A7-14 | P | Tipos públicos do motor construídos por literal fora do crate (`DiagramReplacement` ×3, `OcrOptions` ×7…): a causa do #185. | `apps/pdfcraft-cli/src/main.rs:742,759`; `automation/src/chess.rs:45,415,684` | Q1 |
| A7-08 | I | O fuzz não chega às entradas de xadrez que importam: OCR desligado, pasta de modelos vazia, 4 sementes sintéticas, mutação de bytes que quase nunca gera texto de lances; recompila a CLI release a cada vez (~13 min). | `xtask/src/fuzz.rs:147-245,311,400`; `apps/pdfcraft-cli/src/main.rs:730-796` | Q3 |
| A7-09 | I | Só `ocr_bench --gate` e `chess-bench --allow-lost` falham sozinhos; `walk_bench`, `reference_bench`, `read_bench` só imprimem. | `chessvision/examples/*.rs` | Q2 |
| A7-11 | P | A proptest do walker não sorteia metade das regras (`;`, `a)`, `…`, negrito, legenda, figurinas, `ocr`). | `doc/tests.rs:1645-1734` | Q3 |
| A7-07, A7-15, A7-16, A7-17, A7-18 | P | Dois mutantes sobrevivem em `AdjacentKings`; testes de UI com prazo de relógio de 4–10 s; capturas sem o mutex de GPU; teste de 703 páginas (~70 s) no conjunto padrão; quatro testes que passariam com a função "não fazendo nada". | `analise-9/A7.md` | Q4 |

#### Produto: novas funcionalidades (A8)

| Id | Proposta | Valor · esforço · risco | Passo |
|---|---|---|---|
| A8-02 | Duplo clique num lance impresso no PDF abre a partida no tabuleiro naquele lance (hoje ≥ 3 gestos pela aba Texto); prévia ao passar o mouse. | 5 · S–M · 2 | N1 (MVP) |
| A8-03 | Uma verdade das partidas para **todas** as exportações: texto lido ⊕ PGN do livro ⊕ correções da coleção (usuário > PGN > leitura, com nota). Hoje Word/EPUB/HTML param nos mesmos lances mal lidos. | 5 · M · 2 | N2 (MVP) |
| A8-07 | "Esta posição no livro": onde a posição do tabuleiro aparece e o que o livro joga a seguir, com volta à página. | 4 · M · 2 | N3 (MVP) |
| A8-04 | Solucionador exato de "mate em N" (`pdfcraft_chess`, wasm; 5,6–10,5 ms para refutar um mate em 3 em posições de teste): confere problemas, acha soluções múltiplas (*cooks*), ajuda a decidir quem joga e **valida a leitura** do diagrama. | 4 · M · 1 | N4 |
| A8-06 | A leitura do livro guardada em disco entre sessões (hoje cada reabertura relê tudo; detectar o livro leva ~7 min). | 4 · M–L · 3 | N5 |
| A8-13 | Marcadores do PDF a partir do livro lido: capítulos ▸ partidas ▸ exercícios. | 4 · S–M · 2 | N6 |
| A8-18 / A6-25 | Tabuleiro de análise legível por leitor de tela (grade de casas, lance falado). | 2 · S–M · 1 | N7b |
| A8-11 | Motor com Threads, Hash e Syzygy, só se anunciados; avaliações guardadas. | 3 · S–M · 1 | N8 |
| A8-12 | Das partidas (PGN/coleção) a um livro Word/EPUB/HTML com diagramas nos `$220`. | 4 · M · 2 | N9 |
| A8-05 | Exercícios do livro como perguntas de treino do ChessBase (`%tqu`) e cartões do Anki (TSV + PNG). | 4 · S–M · 3 | N10 (D20) |
| A8-16, A8-14, A8-19, A8-17 | Estudo do lichess (PGN dividido por capítulo); folha de exercícios em PDF; pacote de revisão (exportar/importar correções, decisões e partidas — vai para G5); cada lance como link invisível no PDF. | 2–3 · S–M | N11, N12, G5b, — (A8-17 cortado na v7) |
| A8-08, A8-09, A8-10, A8-15 | Biblioteca de livros com busca entre PDFs; busca por material e estrutura; conferir a análise do livro com o motor em lote (primeiro via ChessBase, depois no app); modo "resolver" no tabuleiro (reabre a decisão de 2026-10-25). | 3–5 · M–L · 3–4 | N15–N18 |

## Anexo B — Histórico de revisões

| Versão | Data | O que mudou |
|---|---|---|
| v0 | 2026-11-11 | Síntese de A1, A2, A4, A5 e A8. |
| v1 | 2026-11-11 | A6 e A7 integrados; spec visual e de acessibilidade (§2.8); passos Q1–Q5, G1–G10, F1–F6, L1–L9, X1, I1–I14, N1–N18; métricas, riscos e decisões D17–D24. |
| v1.1 | 2026-11-11 | A3 (visão) integrado: tabela de achados, passos V1–V9, grafo, ordem e métricas. |
| v2 | 2026-11-11 | **R1 — verdade e precisão** (notas 7·7·8·8·6·8·7; 55 afirmações conferidas, 9 achados reproduzidos pela CLI). Corrigidos: total de linhas (~97 mil; automação 5,3 mil), entradas do parity (20), "CONFIRMADO no app" do A6-01 (o aviso de exportação é pulado, por leitura), números do A2-11 e do A6-07, métrica das perdas (26, separadas em perda, resultado errado e aborto). Spec: o `DocStore` (L6) recebe o gravador atômico em vez de depender de `platform` (L7); duas impressões digitais distintas no glossário; "impressão digital vence índice" em G2 (achado novo R1-01, C); D25 para não reverter em silêncio a decisão do R6e; Job Object fora (exigiria `unsafe`); G6 e G10 sem depender de ondas posteriores; varreduras do Q1 em módulos sem dependência; wasm de engine/automation no portão. Achados novos R1-01 (C), R1-02 (I), R1-03 (P); A4-11 sobe para C; A4-19 na tabela. |
| v3 | 2026-11-11 | **R2 — correção e nunca entrar em pânico** (notas 7·8·6·7·6·6·8; 27 afirmações, 7 falsas; ~14 mil linhas pouco lidas varridas, limpas na UI). O V1 passa a medir também a cadeia de referências do SVG e a rodar o `usvg` com pilha própria (achado novo R2-01, C: SVG plano com `<use>`/`clip-path`/`mask` aborta). `DocStore` separado em `StoreKind` × `StoreBackend` (sem `Access`, que é L7; serve à web). Trava permanente com nome novo (`<kind>.flock`) e a antiga tomada na transição. F1 decide a codificação por linha e mantém o hash legado do PGN guardado. Novo §2.10 (compatibilidade entre versões: formato sobe de versão, cifra grava o agregado antigo, endereços de bloco estáveis com recasamento, teste novo → antigo → novo). F2 com arena e teto de profundidade no `parse`; F3 com um teto só de leitura e escrita e `already_says` única (R2-02); F6 não passa a recusar campo a mais; mecanismo do `Job<T>` por escrito; `read_capped` reaproveitado; `ensure_id` sobre todos os bytes. Achados novos R2-02 (I), R2-03, R2-04, R2-05 (P). |
| v4 | 2026-11-11 | **R3 — arquitetura e ub-mode** (notas 8·7·6·8·6·6·8; 21 afirmações, nenhuma falsa, 7 em parte; 2 achados confirmados por teste). Forma do dado antes da lógica: nova onda **T** (T1 `Fen`/`PositionKey` no L0, substituindo ≥ 12 regras de "mesma posição"; T2 um tipo por endereço e `page_label` único; T3 `AppDirs` em L7) logo depois do Q1; novo **L0** (leitura × walk separados: re-percorrer = ler de novo; achado R3-01); **um** casador correção ↔ diagrama (`assign`) no motor e a UI lendo `Corrected.applied` (sai o `effective` da UI); a coleção de partidas e a cifra viram `StoreKind`s, com `merge` por tipo e um orçamento web só; o F2 reaproveita `FragmentItem` + o núcleo do graft (sem `RavNode`) e o NAG ganha tipo; o F4 escreve as partidas do documento por `to_game`; o I3 primeiro apaga a releitura do autosave (depois do F4) e só então mede; o `role` do bloco só para o que a página decide sozinha; `DiagramReading` com `SideToMove` e orientação; `lichess_url` única; `PageSet` no motor; domínio por documento na sessão do motor, `DocChess` só com a vista; glossário com um tipo por termo. Achados novos R3-01, R3-03, R3-04, R3-05, R3-06 (I) e R3-02, R3-07, R3-08, R3-09, R3-10 (P). |
| v5 | 2026-11-11 | **R4 — UX, design visual e interação** (notas 7·6·5·7·5·4·7; 22 afirmações, 3 falsas ou conflitantes; app rodado no modo Ler). Nova §2.8.0 "Mapa das superfícies" com orçamento de colunas por largura, coluna Xadrez com abas e mestre-detalhe (o mesmo no modo Ler e na janela), tabuleiro sem rolagem e esboços a 1366 × 768; novo passo **I0** (absorve I12, o Original/Lido do I13 e a oferta de recolher Ferramentas). Premissas corrigidas: "Lance nº" do tabuleiro fica (é correção) e muda de lugar e de nome; o egui não tem sublinhado ondulado — a aba Texto separa os dois eixos (estado por cor + marcador; ligação ao PGN pelo sublinhado de hoje); cores de domínio numa lista fixa testada em vez de "nenhuma cor fixa"; Original/Lido no mesmo quadrado, sem empilhar. Novos: I15 (rápidos e visíveis na onda 1, inclusive corrigir no modo Ler — achado R4-01), I16 (xadrez nos menus e na paleta, R4-02), I17 (avisos com Desfazer e `callout`), X2 (diálogo de exportação, R4-04); alvos ≥ 24 pt, dois tamanhos de botão-ícone, um ícone por conceito, escala tipográfica e de espaçamento; tabela exata de textos pt-BR (§2.8.2); decisões de design de N1, N3 e N7; métricas de jornada novas. |
| v6 | 2026-11-11 | **R5 — acessibilidade** (notas 7·5·6·7·6·4·7; 20 afirmações; app a 100 % e 200 %; EPUB, HTML e DOCX exportados e lidos). Alvo declarado (WCAG 2.2 AA no app, EPUB Accessibility 1.1 nas exportações) e nova **§2.8.3 matriz** critério → hoje → passo → teste → TA real; orçamento de colunas em pt lógicos com modo estreito (< 1000 pt) e testes de leiaute a 200 % (achado R5-04); novo **I18** (o escopo de teclas segue o foco, atalhos de um caractere desligáveis, um mapa de teclas único com teste de colisão — achado R5-01; o `?` vira F1 e o `O` vira Alt+O); `announce` com o lance anunciado a cada navegação e as 99 chamadas `notify*` classificadas (R5-02); **N7a** (lance e setas pelo teclado, tabuleiro descrito) sobe para a onda 4 (R5-03, nível A); L6 usa o lado que o walk tira da legenda em vez de "lado não informado" (R5-05: 4 de 6 descrições erradas); novo **X3** (exportações acessíveis, metadados derivados do conteúdo); tabela da aba Texto com os 7 `MoveStatus` (R5-12); modais e foco por inteiro; acessibilidade rápida no I15; `ui inspect` com `live`/`modal`/`bounds` (Q5); D26 (teste com NVDA e Lupa reais). |
| v7 | 2026-11-11 | **R6 — domínio do xadrez** (notas 7·7·7·7·7·5·8; 17 afirmações, 4 erradas ou incompletas; 36 casos pela CLI; solucionador de teste sobre o Polgar). N4 reescrito com as convenções de problema (só `Mate(n)` direto; lado só quando um único lado dá mate; roque permitido salvo prova; "solução múltipla" no lugar de "dual"; `NoMate` com a melhor tentativa e a refutação; n ≤ 3) e o **enunciado** tipado (`Stipulation`) compartilhado com o L6, lido também do cabeçalho corrente; o lado impresso em palavras passa a ser lido (achado R6-03); a meta do L4 deixa de apagar os 17 lances confirmados ou recuperados pelo PGN do livro; o `Result` lido nunca é reescrito (`outcome()` à parte, `ResultMismatch`); `Variant`/Chess960; novo **F7** (comandos `%clk`/`%eval` fora do comentário — achado R6-01 — e dialeto Lichess no escritor, `$201`); `BookKey` no lugar de "mesma posição" (que na FIDE inclui roques e en passant); N3 com continuações conferidas na posição real; L5 com a marca de captura conferida (R6-02); N8 com o histórico de lances para o motor; N10 primeiro em PGN de problemas padrão; N19 lista de erratas; índice de variantes no N6; N14 cortado; D18 vira uma sessão de 15 min no ChessBase; D27 (notação descritiva). |
| v8 | 2026-11-11 | **R7 — segurança e integridade dos dados** (notas 8·7·7·7·6·5·8; 23 afirmações, 2 erradas; modelos ONNX hostis, arquivo travado e saídas colidindo reproduzidos na CLI). **V8 vai para a onda 0** e confere o grafo ONNX antes de `Model::load` (achado R7-01, C: um modelo válido de 1,29 MB aborta o processo); o formato novo das partidas nunca como `*.games` (os builds instalados os poriam de lado); partidas com `uid`/`rev`/lápide, mescla e dono por instância; migração que mescla e pode rodar de novo, com `migrated.json` e `chess_store_migrate`; contrato do pacote de revisão (validação, `pack_id`, importação em estágio, sem caminhos); `persist_policy` única para documento protegido e pasta de dados só do dono; `READER_VERSION` criado e gravado, `reference` sobe para v2; trava antiga enquanto um build antigo puder gravar o tipo; preferências que preservam chaves desconhecidas; testes de compatibilidade com structs serde congeladas e a CLI antiga opcional; `lock` sem `Box<dyn Drop>`; autosave em worker com "último pedido vence" e espera ao sair; troca atômica de conjunto de peças (achado R7-02); saídas distintas e caminhos relativos nas respostas; G9 acha o rótulo antigo pela marca; Redesenhar grava a correção antes do desenho; `setoption` sem quebra de linha; N5 com integridade e espaço livre; biblioteca sem abrir arquivos de nuvem; 7 riscos novos com mitigação conferível. |
| v9 | 2026-11-11 | **R8 — desempenho e escala** (notas 7·6·6·5·7·4·8; medido na CLI release com o Harikrishna, o *Chess Evolution* e um PGN de 1 milhão de lances). Novos **G11** (o livro lido uma vez por sessão: os jobs reliam o livro inteiro a cada exportação, 304 s contra 5,2 s — achado R8-01) e **V10** (varredura em paralelo: hoje 1 núcleo de 6 — R8-02; meta do livro inteiro ≤ 1,5 min); jobs cooperativos no wasm (R8-03: a aba congelava); contrato de custo O(D + C) para `assign` e memo de chaves/impressões que sobrevive à edição (R8-04: 1,7 s a ≈ 73 s depois de apagar uma página; R8-05: 33 ms de rehash por mudar só o Título); a UI nunca espera a trava, e o G3(b) só com o worker do I3; índice do PGN do livro compacto e fora do thread da UI (R8-06: 1,18 GB no teto); etapa 4b "desempenho" no Q1 e §5 com "Como medir" (release, máquina de referência, contagem determinística + tempo contra a base); orçamentos em L0, N3, N4, N5 e N15 (horas, não "L"); metas novas (exportações, DOCX ≤ 25 MB — R8-09 —, autosave com uma partida de 10 mil lances, maior quadro do wasm); recortes corrigidos (≈ 470 MB, não 90); detectores com meta em release; T1 sem custo nos laços quentes; `ChessCache` com LRU por documento; `play` sem clone (R8-08); lista em Colunas virtualizada (R8-07). |
| v10 | 2026-11-11 | **R9 — executabilidade do roadmap** (notas 7·6·8·7·4·4·6; 34 referências cruzadas e 45 âncoras conferidas; um ciclo no grafo, seis dependências invertidas entre ondas, ondas "paralelas" disputando os mesmos arquivos). Nova §3.0 com o Passo 0 e o **núcleo de 10 passos** (≈ 16 PRs, ≈ 17 dias de agente: fecha 3/3 abortos e 18/29 perdas); §3.1 grafo acíclico (G2b → I1a; I2 → I3b → G3b → G2c → G5b; G3a + I3b + V4a → G3c); §3.2 ondas em **três faixas** (A motor/automação, B UI, C `chess`/`chessvision`) com os arquivos de cada célula, caminho crítico (faixa B), estimativa (≈ 110 PRs, ≈ 10 semanas de parede com três faixas) e mapa dos arquivos quentes; nova §3.4 com a ficha de cada passo (onda · faixa · depende de · PRs · formato gravado); §3.3 com Contexto/Saída/Reversão padrão; passos divididos em PRs (Q1a/b, Q3a/b, Q4a/b, T1a/b, T2a/b, G2a/b/c, G3a/b/c, G5a/b, G7a/b, A9-L0a/b, L1a/b, L6a/b/c, X1a/b, I0a/b/c, I3a/b, I11a/b, V4a/b, V6a/b, V10a/b, I7a/b, N5a/b); novo **G12** (versão do `.games` e o `StoredComments` preservado, R9-01) e tarefa do `persist()` no I18; G8 e I15 para a onda 0–1; X1 para a onda 3; "L0" camada × "A9-L0" passo; ids de outros planos qualificados; refs mortas (I12, N7, N14) corrigidas; dados locais de medida com caminho fixo (§0); §7 diz quando perguntar cada decisão (D17, D25, D19 já; D18 e D26 antes da onda 3). |
| v11 | 2026-11-11 | **R10 — excelência, o plano inteiro** (notas 7·8·8·8·7·6·6; 20 afirmações conferidas, 4 reproduções na CLI; roteiro de coerência sobre ficha, grafo, ondas e núcleo). Sumário executivo no topo; dependências numa fonte só (a ficha §3.4; 17 nós corrigidos; legenda do grafo; Q2 antes do Q1b; `coerencia.py` sem divergência nem ciclo; 120 PRs); o #254 do `main` (A6-05, A2-06, A6-10, I8, I13, L1) e a nova linha **Base**; o ciclo F7 ↔ D18 (o kit vai para o F1) e o roteiro do NVDA no I11a; `reader_version` gravado **maior** que o do código abre só para leitura; Reversão em 15 passos com formato gravado; o portão não esconde os seis testes de UI que saem cedo com modelo instalado (R10-05); nova **onda E** (E0 entrega a cada onda, E1 ajuda e guia, E2 builds lado a lado e "Verificar atualizações" do fork) e **D28**; coluna Passo no §5 e passo em cada risco do §6; Passo 0 em dois PRs (P1 feito, P2 notas do `parity/`); L2 abre a onda 3 da faixa C; I18 abre a onda 2 da faixa B; jornada de ouro no Q1b; §7 na ordem dos ids; §1.2 → Anexo A, histórico → Anexo B. Não feito: a coluna de dias por passo (R10, Recomendado 5) e mover §2.8.0/§2.8.3 para anexo (são spec consultada pelos passos I). |

## Anexo C — Registro de ações (ub-mode "Keep a Log")

Ver `analise-9/log.md`.
