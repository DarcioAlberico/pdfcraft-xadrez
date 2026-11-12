# Ajuda do xadrez — o que ficou velho

A ajuda em PDF (`dist/app/Ajuda - Ferramentas de Xadrez.pdf`, 36 pp., 2026-10-31) descreve o build de `cc2d484c`. A fonte HTML e as imagens se perderam numa pasta temporária; o texto está em [`texto-2026-10-31.txt`](texto-2026-10-31.txt), extraído por `pdfcraft-cli text`, e os geradores (`gen_book.py`, `toc.py`, `sheet.py`) estão fora do repositório, em `C:/Python-Chess2/analise-9-local/ajuda/`.

Cada passo do plano A9 (`../plano-analise-9.md`) que muda algo visível acrescenta aqui uma linha com o capítulo que ficou velho. O passo A9-E1 refaz a ajuda e o guia a partir desta lista e a esvazia.

| Passo | PR | Capítulo da ajuda | O que mudou |
|---|---|---|---|
| (#254) | #254 | 2 — O que é preciso: modelos, fontes e motor | O motor numa pasta dentro de `engine/` passou a ser achado; não é mais preciso copiar o `.exe` direto para `dist/app/engine`. |
| A9-G12 | #259 | 13 — As partidas do documento (Salvamento automático) | Um arquivo de partidas gravado por um PdfCraft mais novo (versão maior) não é mais renomeado `….games.bad`: as partidas do próprio documento são só lidas, com aviso e a nota "Salvas por um PdfCraft mais novo: aqui estas partidas são só lidas." na lista; os arquivos de outros livros nunca são tocados. |
| A9-V1 | (este PR) | 15 — Aparência: peças, temas e lista (Importar pasta…, Importar de página web…) | Uma peça SVG aninhada ou encadeada mais de 32 níveis, que se refere a si mesma ou que se expandiria além de 200.000 elementos é recusada com o motivo (antes fechava o programa). |
| A9-G8a | (este PR) | 12 — Variantes, estrutura e Desfazer ("Lances apagados nunca se perdem") | A partida como estava vai para a lixeira com o próprio passo que tirou os lances, com qualquer painel à vista: um lance excluído na Lista de lances ao lado da página e um PGN mais longo carregado do campo por cima da partida também contam. |
| A9-G8b | (este PR) | 13 — As partidas do documento (Salvamento automático) | O diagrama aberto e não mexido cede o lugar ao próximo diagrama, não conta no limite de 256 nem é salvo; acrescentar partidas além de 256 é recusado com aviso; Sair, Fechar e Fechar todos perguntam “Partidas de xadrez não salvas” (Salvar todas as partidas em PGN… · Sair/Fechar sem salvar · Cancelar) quando o salvamento automático não guarda as partidas. |
