# A ajuda do xadrez em PDF

**Status:** a fonte da ajuda é [`ajuda.html`](ajuda.html), em português do Brasil, refeita no passo A9-E1 do plano (`../plano-analise-9.md`) e no A9-E1c para o build que a capa nomeia. O PDF, as figuras e os geradores ficam fora do repositório.

| Data | Mudança |
|---|---|
| 2026-11-18 | A9-E1: a ajuda refeita para o build `f720627`, com a fonte no repositório. |
| 2026-11-19 | A9-E1c: a Biblioteca (N15c) e o I6b; as capturas refeitas do build `7496134`, o do `dist/app`. |

## O que fica onde

| O quê | Onde |
|---|---|
| A fonte (texto, sem imagens) | `docs/chess/ajuda/ajuda.html`, neste repositório |
| As figuras (`img/*.png`) | `C:/Python-Chess2/analise-9-local/ajuda/img/` |
| O livro de exemplo (`livro-exemplo.pdf` e `.pgn`) e o gerador `gen_book.py` | `C:/Python-Chess2/analise-9-local/ajuda/livro/` e `…/ajuda/gen_book.py` |
| O build: `build.py` (imprime e numera o sumário), `toc.py` (as páginas do sumário pelo `pdfcraft-cli text`), `sheet.py` (folhas de miniaturas para conferir as páginas) | `C:/Python-Chess2/analise-9-local/ajuda/` |
| A cópia portátil do app usada nas capturas (`portable.txt` ao lado do exe) | `C:/Python-Chess2/analise-9-local/ajuda/app/` |
| O PDF gerado | `C:/Python-Chess2/analise-9-local/ajuda/out/Ajuda - Ferramentas de Xadrez.pdf`; a entrega do fim da onda (A9-E0) o copia para `dist/app` |
| O texto da ajuda anterior (31/10, build `cc2d484c`), para comparar | [`texto-2026-10-31.txt`](texto-2026-10-31.txt) |

Nenhuma imagem, fonte ou PDF entra no repositório (`AGENTS.md` §1). O `ajuda.html` refere as figuras pela pasta relativa `img/`, que só existe na pasta do build.

## Refazer o PDF

1. Gere o livro de exemplo, se ele não existir: `py -3.11 -I gen_book.py livro/livro-exemplo.html <ChessMerida.ttf>` (precisa do `python-chess`; a fonte está em `dist/app/fonts`) e imprima o HTML com o Chrome em `livro/livro-exemplo.pdf`.
2. Tire as capturas de um build do `main` (release com `PDFCRAFT_BUILD_ID="<sha curto> <data>"`), numa cópia portátil com `portable.txt`: `pdfcraft.exe --control FILE livro-exemplo.pdf` e `pdfcraft-cli ui --control FILE screenshot|click|drag|set|command|resize`. A janela fica visível enquanto é comandada; interface em português; 1366 × 768. As Preferências só rolam arrastando a barra de rolagem. Ponha o build na legenda de cada figura.
3. Rode `py -3.12 -I build.py <repo>/docs/chess/ajuda/ajuda.html <pdfcraft-cli.exe>`. Ele copia a fonte para a pasta do build, imprime com o Chrome (`--headless=new --no-pdf-header-footer --generate-pdf-document-outline`, que faz os marcadores a partir dos títulos), preenche os números de página do sumário com o `toc.py` e imprime de novo até os números pararem de mudar.
4. Confira cada página (`sheet.py` faz folhas de miniaturas) e se os marcadores existem.

## As regras da fonte

- Um nome da interface vai em `<b>`, exatamente como `crates/ui-egui/src/i18n/pt-br.tsv` o tem (um `{marcador}` do catálogo pode ser preenchido). Ênfase usa `<strong>` ou `<em>`, nunca `<b>`. O teste `i18n::tests::the_help_names_only_what_the_app_shows` confere.
- O glossário do plano (§2.1, §2.8.2) vale frase a frase: diagramas são **detectados** (nunca "procurar", "busca" ou "varrer" numa frase sobre diagramas); o símbolo de um lance é **sinal** ("glifo" só para os caracteres que o OCR lê, com modelo, leitor, fonte ou OCR na mesma frase); nenhum rótulo que o §2.8.2 trocou ("Ligar o livro", "Encaixar", "Trazer de volta", "Diagramas de xadrez", "Exportar assim mesmo"). O teste `i18n::tests::the_chess_texts_keep_the_glossary` confere.
- Um passo que muda algo visível acrescenta uma linha a [`PENDENTE.md`](PENDENTE.md), ou corrige a ajuda no mesmo PR.
