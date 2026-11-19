# Ajuda do xadrez — o que ficou velho

A ajuda em PDF (`Ajuda - Ferramentas de Xadrez.pdf`) descreve o build que a capa dela nomeia: `7496134 2026-11-19`, refeita no passo A9-E1 (e no A9-E1c, com a Biblioteca). A fonte é [`ajuda.html`](ajuda.html), nesta pasta; [`README.md`](README.md) diz como refazer o PDF e onde ficam as figuras e os geradores. O texto da ajuda anterior (31/10, build `cc2d484c`) fica em [`texto-2026-10-31.txt`](texto-2026-10-31.txt).

Cada passo do plano A9 (`../plano-analise-9.md`) que muda algo visível corrige `ajuda.html` no mesmo PR ou acrescenta aqui uma linha com o capítulo que ficou velho (os capítulos são os da ajuda refeita). A próxima revisão da ajuda refaz as capturas a partir desta lista e a esvazia.

| Passo | PR | Capítulo da ajuda | O que mudou |
|---|---|---|---|
| A9-X1b | (este PR) | 18. Exportar o livro (O que vai e o que não vai para o livro exportado) | O Word leva cada diagrama em SVG, que o Word 2016 em diante mostra e imprime nítido, e numa imagem de 432 px e 64 cores para quem não lê o SVG (impressa a 144 ppi); a mesma posição vai uma vez no arquivo. Um livro de 748 páginas caiu de 118,5 para 21,2 MB. Além de 30.000 posições (ou 1 GiB de imagens) num arquivo, o diagrama sai com a FEN e o aviso da exportação diz quantos. A interface não muda. |
