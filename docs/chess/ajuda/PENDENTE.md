# Ajuda do xadrez — o que ficou velho

A ajuda em PDF (`dist/app/Ajuda - Ferramentas de Xadrez.pdf`, 36 pp., 2026-10-31) descreve o build de `cc2d484c`. A fonte HTML e as imagens se perderam numa pasta temporária; o texto está em [`texto-2026-10-31.txt`](texto-2026-10-31.txt), extraído por `pdfcraft-cli text`, e os geradores (`gen_book.py`, `toc.py`, `sheet.py`) estão fora do repositório, em `C:/Python-Chess2/analise-9-local/ajuda/`.

Cada passo do plano A9 (`../plano-analise-9.md`) que muda algo visível acrescenta aqui uma linha com o capítulo que ficou velho. O passo A9-E1 refaz a ajuda e o guia a partir desta lista e a esvazia.

| Passo | PR | Capítulo da ajuda | O que mudou |
|---|---|---|---|
| (#254) | #254 | 2 — O que é preciso: modelos, fontes e motor | O motor numa pasta dentro de `engine/` passou a ser achado; não é mais preciso copiar o `.exe` direto para `dist/app/engine`. |
| A9-G12 | (este PR) | 13 — As partidas do documento (Salvamento automático) | Um arquivo de partidas gravado por um PdfCraft mais novo (versão maior) não é mais renomeado `….games.bad`: as partidas do próprio documento são só lidas, com aviso e a nota "Salvas por um PdfCraft mais novo: aqui estas partidas são só lidas." na lista; os arquivos de outros livros nunca são tocados. |
