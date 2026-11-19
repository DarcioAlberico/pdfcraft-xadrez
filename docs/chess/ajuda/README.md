# A ajuda do xadrez em PDF

**Status:** a fonte da ajuda é [`ajuda.html`](ajuda.html), em português do Brasil, refeita no passo A9-E1 do plano (`../plano-analise-9.md`), no A9-E1c, no A9-E1d e na revisão da versão 1.1.0, para a versão e o build que a capa nomeia. O PDF, as figuras e os geradores ficam fora do repositório.

| Data | Mudança |
|---|---|
| 2026-11-18 | A9-E1: a ajuda refeita para o build `f720627`, com a fonte no repositório. |
| 2026-11-19 | A9-E1c: a Biblioteca (N15c) e o I6b; as capturas refeitas do build `7496134`, o do `dist/app`. |
| 2026-11-19 | A9-E1d: a ajuda da versão 1.0.0 (build `bc3f895`): os comandos do comentário escondidos e os selos do relógio e da avaliação (F7), **PGN para** o lichess ou o ChessBase (F7), o Word menor (X1b), a pergunta de treino do ChessBase nos exercícios (N10c), as coleções de partidas (#434) e onde ver a versão; todas as capturas refeitas desse build. |
| 2026-11-19 | A ajuda da versão 1.1.0 (build `7233142`): o tamanho do tabuleiro (#448), que já tinha o texto do capítulo 7, entra nas figuras (o menu ⋯ do tabuleiro com **Tabuleiro maior**, **Tabuleiro menor** e **Tamanho do tabuleiro: automático**, e uma figura nova do tabuleiro aumentado, com a alça); todas as capturas refeitas desse build. |

## O que fica onde

| O quê | Onde |
|---|---|
| A fonte (texto, sem imagens) | `docs/chess/ajuda/ajuda.html`, neste repositório |
| As figuras (`img/*.png`) | `C:/Python-Chess2/analise-9-local/ajuda/img/` |
| O livro de exemplo (`livro-exemplo.pdf` e `.pgn`) e o gerador `gen_book.py` | `C:/Python-Chess2/analise-9-local/ajuda/livro/` e `…/ajuda/gen_book.py` |
| O build: `build.py` (imprime e numera o sumário), `toc.py` (as páginas do sumário pelo `pdfcraft-cli text`), `sheet.py` (folhas de miniaturas para conferir as páginas) | `C:/Python-Chess2/analise-9-local/ajuda/` |
| A cópia portátil do app usada nas capturas (`portable.txt` ao lado do exe) | `C:/Python-Chess2/analise-9-local/ajuda/app/` |
| As capturas por script (desde o A9-E1d): `fresh.py` (dados novos na cópia portátil, as preferências mantidas), `shots.py` (cada figura, na ordem, pelo canal de controle; `all` ou `from:<passo>`), `ctl.py` (o cliente do canal), `lichess-exemplo.pgn` e `lichess_game.py` (a partida do capítulo 10), `dialects.py` (o exemplo do **PGN para**), `contact.py` (uma folha com as figuras) | `C:/Python-Chess2/analise-9-local/ajuda/capturas/` |
| O PDF gerado | `C:/Python-Chess2/analise-9-local/ajuda/out/Ajuda - Ferramentas de Xadrez.pdf`; a entrega do fim da onda (A9-E0) o copia para `dist/app` |
| O texto da ajuda anterior (31/10, build `cc2d484c`), para comparar | [`texto-2026-10-31.txt`](texto-2026-10-31.txt) |

Nenhuma imagem, fonte ou PDF entra no repositório (`AGENTS.md` §1). O `ajuda.html` refere as figuras pela pasta relativa `img/`, que só existe na pasta do build.

## Refazer o PDF

1. Gere o livro de exemplo, se ele não existir: `py -3.11 -I gen_book.py livro/livro-exemplo.html <ChessMerida.ttf>` (precisa do `python-chess`; a fonte está em `dist/app/fonts`) e imprima o HTML com o Chrome em `livro/livro-exemplo.pdf`.
2. Tire as capturas de um build do `main` (release com `PDFCRAFT_BUILD_ID="<sha curto> <data>"`), numa cópia portátil com `portable.txt`: `pdfcraft.exe --control FILE livro-exemplo.pdf` e `pdfcraft-cli ui --control FILE screenshot|click|drag|set|command|resize`. A janela fica visível enquanto é comandada; interface em português; 1366 × 768. As Preferências só rolam arrastando a barra de rolagem. Ponha o build na legenda de cada figura. Os scripts de `capturas/` fazem isso: com o app fechado, `py -3.12 -I capturas/fresh.py <pasta da ajuda>`; depois `app/pdfcraft.exe --control control.json livro/livro-exemplo.pdf` e `py -3.12 -I capturas/shots.py control.json img all`. Um passo que falha pode ser refeito com `from:<passo>`, sobre o estado em que o app ficou.
3. Rode `py -3.12 -I build.py <repo>/docs/chess/ajuda/ajuda.html <pdfcraft-cli.exe>`. Ele copia a fonte para a pasta do build, imprime com o Chrome (`--headless=new --no-pdf-header-footer --generate-pdf-document-outline`, que faz os marcadores a partir dos títulos), preenche os números de página do sumário com o `toc.py` e imprime de novo até os números pararem de mudar.
4. Confira cada página (`sheet.py` faz folhas de miniaturas) e se os marcadores existem.

## As regras da fonte

- Um nome da interface vai em `<b>`, exatamente como `crates/ui-egui/src/i18n/pt-br.tsv` o tem (um `{marcador}` do catálogo pode ser preenchido). Ênfase usa `<strong>` ou `<em>`, nunca `<b>`. O teste `i18n::tests::the_help_names_only_what_the_app_shows` confere.
- O glossário do plano (§2.1, §2.8.2) vale frase a frase: diagramas são **detectados** (nunca "procurar", "busca" ou "varrer" numa frase sobre diagramas); o símbolo de um lance é **sinal** ("glifo" só para os caracteres que o OCR lê, com modelo, leitor, fonte ou OCR na mesma frase); nenhum rótulo que o §2.8.2 trocou ("Ligar o livro", "Encaixar", "Trazer de volta", "Diagramas de xadrez", "Exportar assim mesmo"). O teste `i18n::tests::the_chess_texts_keep_the_glossary` confere.
- Um passo que muda algo visível acrescenta uma linha a [`PENDENTE.md`](PENDENTE.md), ou corrige a ajuda no mesmo PR.

## A edição em inglês

- **A fonte** é [`help-en.html`](help-en.html) (passo A9-E1e): a mesma ajuda, em inglês, com os mesmos 21 capítulos, as mesmas 35 figuras e a mesma estrutura. Um nome da interface vai em `<b>` exatamente como o texto-fonte inglês do catálogo o tem (a segunda coluna de `pt-br.tsv`, o texto das chamadas `tl!()`). O teste `i18n::tests::the_english_help_names_only_what_the_app_shows` confere. Nele e no do português, desde o A9-E1e, um texto do catálogo feito só de marcadores (`{n} {kind}`) não vale como nome: antes, ele aceitava qualquer nome com um espaço. O `the_chess_texts_keep_the_glossary` confere a prosa em inglês, frase a frase: nada de "search" ou "scan" numa frase sobre diagramas, "glyph" só junto de OCR, modelo, leitor ou fonte (os nomes em `<b>` e o `<code>` ficam fora, porque a interface inglesa chama os sinais de *glyphs*: **Glyph palette**) e nenhum rótulo inglês que o §2.8.2 trocou.
- **O PDF**, `Help - Chess Tools.pdf`, sai de uma cópia própria do pipeline, em `C:/Python-Chess2/analise-9-local/ajuda-en/`: `build.py` (`py -3.12 -I build.py <repo>/docs/chess/ajuda/help-en.html <pdfcraft-cli.exe> [--out NOME]`; o padrão é `out/Help - Chess Tools.pdf`), `toc.py`, `sheet.py` (`py -3.11`, que tem o Pillow), `gen_book.py` e `livro/` (o livro de exemplo em inglês, `sample-book.pdf` e `.pgn`, com as mesmas partidas e diagramas do português), `img/`, `library/` (a pasta da Biblioteca das figuras 29 e 30) e a cópia portátil `app/`, com a interface em inglês. `shots.sh` (com `ui.sh`) refaz as 35 capturas numa cópia portátil limpa: as do português, com os rótulos e as coordenadas da interface inglesa.
- **A regra:** um passo que muda algo visível corrige `ajuda.html` e `help-en.html` no mesmo PR, ou acrescenta a [`PENDENTE.md`](PENDENTE.md) uma linha que nomeia os dois arquivos.
