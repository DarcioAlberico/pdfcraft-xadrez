# Briefing comum — Análise 9 do PdfCraft Xadrez (2026-11-11)

Você é um revisor sênior, rigoroso, que preza a excelência. Sua parte é UMA área do programa (veja o seu prompt). O resultado vira um roadmap + spec de implementação em pt-BR.

## O programa
- Repositório `DarcioAlberico/pdfcraft-xadrez`: um clone do PdfCraft (Rust, egui, editor de PDF tipo Acrobat) com ferramentas de **livros de xadrez em PDF**: detectar diagramas e ler a posição (FEN), ler o texto de xadrez (lances, figurinas, sinais do Informator) e montar as partidas, revisar/corrigir diagramas, tabuleiro de análise com motor UCI do usuário, lista de lances em árvore, captura de texto do PDF para PGN, coleção de partidas, PGN do próprio livro (Chessable/Forward Chess) alinhado ao PDF, exportação PGN/EPUB/HTML/Word/RTF/Markdown, redesenhar diagramas no PDF, links do lichess.
- **Cópia de leitura:** `C:/Python-Chess2/wt-a9` (worktree no `origin/main` cc2d484c, 2026-10-31). Cite sempre `caminho:linha` desse commit.
- Crates: `crates/chess` (L0: regras, SAN, árvore de jogo, PGN, NAG, reparo, desenho, describe, origin), `crates/chessvision` (visão: detecção, leitura de tabuleiro, glifos/OCR; texto→partidas: `doc/`, `chess_text/`; exportações `doc/html.rs`, `markdown.rs`, `pgn.rs`, `svg`), `crates/engine/src/chess*` (L6: sessão, varredura e cache por página, correções, PGN do livro, captura, análise UCI, redesenhar, coleção, EPUB/Office), `crates/automation/src/chess*.rs` (ferramentas headless), `crates/export` (DOCX/RTF/flow), `crates/ui-egui/src/chess_*.rs` (34 arquivos, ~25 mil linhas), testes em `crates/*/tests/chess*.rs`.
- Regras da casa (leia `C:/Python-Chess2/wt-a9/CLAUDE.md` e `AGENTS.md` §4 se tocar nisso): **nunca entrar em pânico** com entrada não confiável (PDF, PGN, FEN, imagem, modelo, JSON, teclado) — sem `unwrap`/`expect`/indexação com posição vinda da entrada/aritmética sem checagem/alocação sem teto/recursão sem limite; camadas (nada abaixo de L7 usa egui); `chess`/`chessvision`/`engine` compilam para wasm32; toda função nova tem ferramenta headless em `automation` e entrada em `parity/acrobat-features.toml`; ferramentas confinadas a `--root`; sem rede de saída; sem código GPL/AGPL; salvar é incremental; nunca descartar/reparar dado em silêncio.
- Histórico recente: o plano A8 (`C:/Python-Chess2/wt-a9/docs/chess/plano-analise-8.md`) está todo feito (#194–#225); depois vieram PGN do livro R0–R6 (`docs/chess/plano-pgn-do-livro.md`, #229–#245), conjuntos de peças/tabuleiros (`docs/chess/plano-pecas-tabuleiros-lances.md`, HC0–HC13, #239, #247), destaque do último lance #246, largura do tabuleiro e fonte da lista de lances #248, quebra de linha da aba Texto #249. Guia do usuário: `docs/chess.md`.
- **Pendências já conhecidas** (não as "descubra" de novo como novidade; pode confirmá-las, priorizá-las ou mostrar que estão erradas): `C:/Users/AMD/AppData/Local/Temp/claude/0beba26efa9526bf/a8b02681-59f3-4016-8f89-d91a29dba7d7/scratchpad/parity_open.txt` (notas "Missing" do parity). Também em aberto: a aba Diagrama transborda a 1440 px; caracteres crus de diagrama em fonte Merida aparecem como linhas de texto na aba Texto; `dist/app/engine` com o Stockfish numa subpasta não é achado (só `.exe` direto em `engine/`); o fork **não tem CI** (só portões locais).
- Métricas já medidas (ub-mode): complexidade ciclomática por função em `.../scratchpad/metrics/crap.txt` (sem cobertura; 347 funções com CC ≥ 10) e duplicação em `.../scratchpad/metrics/dry.txt` (mesma pasta do `parity_open.txt`).

## Lente obrigatória: ub-mode
Além da lente da sua área, julgue o código por estes princípios e cite-os quando um achado vier deles: *Get the data shape right* · *One decision lives in one place* · *Model the domain: illegal states impossible* · *Check data where it enters; inside, trust it; business logic in pure functions* · *Make operations idempotent (twice, or after a crash halfway, same state)* · *If a concurrent writer would matter, don't share state; one writer enforced by structure* · *Fix root causes, not guards* · *Test behaviour, not implementation (a test that would pass if every function returned nothing is worthless)* · *Subtract before you add* · *Redesign from first principles instead of bolting on*.

## Como confirmar
- Prefira **confirmar** a suspeitar. Formas baratas:
  1. CLI release já compilada do `main` atual: `"C:/Python-Chess2/Chess PDF-Craft/dist/app/pdfcraft-cli.exe"` (`tools` lista as ferramentas; `run --script steps.json --root DIR`, o script é uma LISTA JSON de passos `{"tool": ..., "args": {...}}` — confira o formato com `tools`/o código de `apps/pdfcraft-cli`). Modelos: `PDFCRAFT_MODELS="C:/Python-Chess2/Chess PDF-Craft/dist/app/models"`. Trabalhe só em pastas da sua scratchpad (`.../scratchpad/<seu-id>/`); nunca grave em `dist/app`.
  2. Teste temporário: só nos crates `pdfcraft-chess` e `pdfcraft-chessvision`, como arquivo NOVO de nome único `crates/<crate>/tests/zz_a9_<seu-id>.rs` em `C:/Python-Chess2/wt-a9`, com `CARGO_TARGET_DIR=C:/Python-Chess2/cargo-targets/a9-<seu-id>`, `CARGO_INCREMENTAL=0`, `-j 2`, `timeout 900`. Apague o arquivo ao terminar. Não edite nenhum arquivo existente do worktree. Nada de build de `ui-egui`, `engine`, `automation` nem do workspace inteiro (a máquina trava).
  3. Leitura cuidadosa com o caminho do dado da entrada até o efeito.
- Marque cada achado como **CONFIRMADO** (diga como: comando/teste e saída resumida) ou **PLAUSÍVEL** (diga o que falta para confirmar).
- Scripts Python: escreva-os com a ferramenta Write num arquivo (heredocs do Bash comem barras invertidas); rode com `py -3.12 -I`.
- Livros de teste do usuário (não copiar texto deles para o relatório além de citações mínimas): `C:/Users/AMD/Documents/PDFCraft_Arquivos/PDF e PGN/` (Harikrishna, *Beat the French Defence with 3.Nc3*, 748 pp, com o PGN do livro) e `C:/Users/AMD/Documents/PDFCraft_Arquivos/*Chess Evolution*.pdf`. Copie para a sua scratchpad antes de usar.
- Proibido: commit, push, `gh`, apagar arquivos que não criou, rede (salvo se o seu prompt permitir pesquisa web), instalar programas.

## Formato do relatório (arquivo `.../scratchpad/findings/<seu-id>.md`, pt-BR, identificadores de código em inglês)
Cabeçalho com escopo, o que leu/rodou, e então de 15 a 40 achados, os mais graves primeiro:

```
### <seu-id>-NN · [C|I|P] · [BUG|RISCO|MELHORIA|NOVA|DÍVIDA] · Título curto
- **Evidência:** `crates/...:linha` (+ CONFIRMADO: como / PLAUSÍVEL: o que falta)
- **Cenário:** entrada/estado concretos → resultado errado/impacto para o usuário
- **Proposta:** a menor mudança que resolve a CAUSA; o teste que deve falhar antes; ferramenta headless/parity afetados
- **Esforço:** S (<150 linhas) / M (<600) / L; arquivos tocados; dependências de outros achados
```
C = crítico (perda de dados, pânico, resultado errado silencioso, segurança) · I = importante · P = polimento.
Seja concreto e verificável; nada de conselho genérico ("adicionar mais testes"). Não infle: um achado fraco rebaixa o relatório inteiro. Termine com "Pontos fortes que não se deve quebrar" (3–6 itens) e "O que não verifiquei".

Sua resposta final (para quem o chamou) é um resumo de no máximo 250 palavras: os 5 achados mais graves em uma linha cada e o caminho do arquivo.
