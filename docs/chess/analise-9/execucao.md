# Plano A9 — registro de execução

**Status:** em execução desde 2026-11-11 · núcleo (§3.0) completo em 2026-11-12 · ondas 0 e 1 completas, onda 2 em andamento.

Cada passo do plano (`../plano-analise-9.md`) com o PR que o fez e o que mudou em relação ao texto do plano. O plano continua sendo a fonte do *que* fazer; este registro é a fonte do *que foi feito* e das decisões tomadas no caminho, para que o próximo passo não precise redescobri-las. Cada PR traz no corpo o teste visto falhando com a correção revertida e o resumo do `cargo xtask chess-gate`.

## Ondas P, 0 e 1 (o núcleo)

| Passo | PR | Desvios e decisões |
|---|---|---|
| P1 | #255, #256 | O plano, os relatórios e o texto da ajuda no repositório; D17, D25, D19 e D28 respondidas (sim às quatro). |
| P2 | #258 | Frase "Missing (plan A9, …)" em 18 das 20 entradas de xadrez do `parity/`. |
| Q1a | #261 | `cargo xtask chess-gate`, etapas 0–3; `known-failures.toml`; `test_models()`/`test_engine()` com `PDFCRAFT_REQUIRE_MODELS` (adiantados do Q1b); os seis testes de UI fixam "sem modelo" no harness; os 13 escapes `\x27` do `parity/` viraram apóstrofos. |
| Q1b | #280 | Construtores para os quatro tipos do A7-14, etapas 4/4b/5, a jornada de ouro (`tests/chess_golden.rs`), `--baseline` em `cargo-targets/gate`. Ainda sem teste `perf_`, então a etapa 4b só conferiu o próprio parser. |
| Q2 | #278 | Uma regra de casamento de lances para os benches (`examples/common/moves_match.rs`). |
| Q4a | #283 | `rules.rs`: 42 de 42 mutantes mortos; `Square` total; `cargo xtask mutate`. |
| G1 | #265 (G1a) | **Dividido.** G1a: `/ID` sobre todos os bytes escritos, veto do `id0`, `/ID` constante tratado como ausente, identificador do EPUB com o intervalo, nota para o upstream (`docs/chess/upstream.md`). **G1b** (separar os stores antigos chaveados pelo `/ID` constante que misturam livros) **foi para a migração do G5a** (onda 3), com três decisões: separar ao gravar ou migrar, quando as impressões digitais das páginas existem (nunca na carga); as entradas de outro livro listadas por `chess_correction_list` com `other_book: true`; uma parte vê as suas entradas (casadas pela impressão digital) assim que as impressões existem. Até lá a parte cujo caminho o arquivo compartilhado registra fica com tudo, e nada é apagado. |
| G2a | #273 | `corrections` v2: as impressões digitais das páginas vizinhas e `Located::Orphan`; o portão passa a CLI instalada como `PDFCRAFT_OLD_CLI` para os testes de compatibilidade. |
| G7a | #268 | Confinamento e sobrescrita; o roteiro `s13.json` (o `chess_games` cortando partidas) ficou aqui, não no G8. |
| G8 | #262, #266, #267 | **Dividido em G8a/G8b/G8c.** G8a: a lixeira alimentada pelo próprio passo de edição; "mesma partida" por um hash do conteúdo (não pela revisão, que muda ao restaurar do autosave). G8b: recusar acima do teto com aviso; a partida de diagrama intocada é passageira (substituí-la não é passo de desfazer, e ela não volta na sessão seguinte; reabre-se do diagrama); Sair/Fechar perguntam por documento, como os PDFs não salvos: **Salvar todas como PGN…** (que cancela a saída), **Sair sem salvar**, **Cancelar**; na web, só o aviso no teto. G8c: o aviso antes de gravar partidas lidas só até um lance impossível (a marca "cortada" não sobrevive a reiniciar). |
| G11 | #275, #276 | As duas partes (motor e interface) feitas pela faixa A. |
| G12 | #259 | Amostra v1 congelada em `ui-egui/tests/compat/games_v1.games`. |
| E2 | #271 | Socket por pasta do executável; id do build no Sobre, no `--version` e no `chess_status` (`PDFCRAFT_BUILD_ID`); "Verificar atualizações" do fork não usa a rede. |
| F1 | #269 | Também fechou o A4-08 do G4 (recarregar o mesmo PGN do livro mantém as decisões), porque o teste do F1 precisava; `bytes_hash` não é gravado (fica para o G4), então o formato não mudou. O kit da D18 está em `docs/chess/d18-kit/` (não em `analise-9/d18/`); a linha `%tqu` é um palpite a conferir na sessão. |
| F2a, F2b, F3 | #270, #277, #281 | Um scanner, o leitor em três passos (scan, itens do graft, build), nada cortado em silêncio. |
| V1 | #260 | Profundidade e cadeias de referências medidas antes do `usvg`; também recusa laços, expansão acima de 200 mil elementos e seletores longos. |
| V6a | #282 | Troca atômica do conjunto de peças. |
| V8 | #264 | O grafo ONNX conferido antes do `rten` (operadores, constantes e tamanho de todo tensor) e a paridade na carga. |
| I15, I7a | #272, #274 | O editor do modo Ler ganhou o botão **Confirmar** (sem ele o R4-01 não fechava). O botão da lista ficou "Save all games as PGN…", o nome que a ação já tinha, em vez do texto da §2.8.2. Dois alvos pequenos a mais corrigidos ("Seguir a página" e o link do título da aba Diagrama). Ctrl+Enter, PgUp/PgDn e Esc ainda não funcionam no editor do modo Ler. |

## Onda 2 (em andamento)

| Passo | PR | Desvios e decisões |
|---|---|---|
| I18 | #279 | A aba Diagrama **não** entra no escopo de teclas do tabuleiro (← moveria a partida e o Ctrl+Z cairia no A5-06); clicar em menus e barras não encerra o escopo (Editar ▸ Desfazer depois de um lance desfaz o lance); a checagem de foco pula os quadros de clique. A suíte completa da UI passou (1.718 testes). |
| A9-L0 | #289 (L0a), #291 (L0b) | **L0a:** a lance guarda a leitura (`san_read`, `read_review`) ao lado do que o walk decidiu, que continua no `MoveInline` (o JSON do `chess_document`, as exportações e a aba Texto o leem ali; separar o tipo de saída fica para depois do I1a); `doc::walk(pages, rules)`; `READER_VERSION = 1` só como constante. **L0b, três decisões do coordenador (2026-11-12):** (1) o `reading` de um diagrama confirmado continua sendo reescrito pelo `link`, como antes, até o L6b; o L0b só re-percorre as partidas com os diagramas confirmados depois do `link` e alinha de novo uma vez (o A2-11, que era do L8). (2) Uma decisão casa pelo endereço **e** pelo `san_read` quando o guardou (senão pela SAN do walk, como antes): mais seguro, nunca cai noutro lance; recasar só pelo `san_read`, com a reversão que precisa do `reader_version` no arquivo, é do L1b (depois do G3a). (3) O orçamento por fronteira de página (estado do walker guardado por página, re-percorrer só a partir da partida afetada) vai para o **A9-L9** (desempenho do walk); o `link` re-percorre o documento inteiro, só quando uma confirmação mudou uma leitura. |

## Onda 3 (em andamento)

| Passo | PR | Desvios e decisões |
|---|---|---|
| A9-L2 | #293 | O texto entre um diagrama e o lance fica guardado (`Held`, com o teto de 64 KiB dos comentários) e é lido no lugar quando a partida continua (colchetes e resultado contam), ou vira o comentário inicial da partida que começa no diagrama. **Decisões:** (1) a deixa de uma legenda "Position after" sai com a frase dela quando o lance é lido como legenda **ou** quando a partida aberta o joga (o diagrama reimprime a posição e a legenda nomeia o próximo lance); (2) o texto depois das variantes de um lance vira `comment_before` do próximo lance (A2-02, parte do walker) e, quando o PGN não tem lugar para ele (as variantes não entraram na árvore, ou a ordem das linhas mudou), vai para o comentário do lance anterior ao fechar a partida (`settle_comments_before`), para que o PGN do documento releia igual; o leitor PGN do documento (só de testes, A2-18) lê assim também; o escritor do tabuleiro (`chess::game`) continua no F4; (3) não feito: o texto depois de um diagrama da posição final, sem lance depois (nenhum lance o resolve: continua só no documento), e a contagem de "texto não atribuído" no resumo do `chess_document` que o A2 propôs. |

## Fora do plano, no mesmo período

- #257: o modelo de peças retreinado no ChessVisionOFF (`fccd0097`) e o limiar de aceite por modelo (`accept_min_confidence`).
- Falha intermitente conhecida: `pdfcraft-ui-egui --test chess_theme` morreu uma vez com access violation ao renderizar capturas e passou ao repetir; está em `xtask/known-failures.toml` como `flaky`.
