# Ajuda do xadrez — o que ficou velho

A ajuda em PDF (`Ajuda - Ferramentas de Xadrez.pdf`) descreve o build que a capa dela nomeia: `f720627 2026-11-18`, refeita no passo A9-E1. A fonte é [`ajuda.html`](ajuda.html), nesta pasta; [`README.md`](README.md) diz como refazer o PDF e onde ficam as figuras e os geradores. O texto da ajuda anterior (31/10, build `cc2d484c`) fica em [`texto-2026-10-31.txt`](texto-2026-10-31.txt).

Cada passo do plano A9 (`../plano-analise-9.md`) que muda algo visível corrige `ajuda.html` no mesmo PR ou acrescenta aqui uma linha com o capítulo que ficou velho (os capítulos são os da ajuda refeita). A próxima revisão da ajuda refaz as capturas a partir desta lista e a esvazia.

| Passo | PR | Capítulo da ajuda | O que mudou |
|---|---|---|---|
| A9-N15c | (ainda não no `main`) | 20 — Automação pela linha de comando (a biblioteca); provavelmente um capítulo novo | O diálogo da Biblioteca no app (a busca do N15b numa janela) não tinha entrado quando a ajuda foi refeita: a ajuda só cita as ferramentas `chess_library_build`, `_find`, `_status` e `_clear` e diz que o app ainda não tem o painel. |
| A9-I6b | #432 | 15 — As partidas do documento (Dados da partida; O menu de uma partida); 7 — O tabuleiro de análise (o quadro vermelho da posição ilegal) | Uma posição ilegal no tabuleiro não tem partida: os campos de **Dados da partida** e o **Renomear…** da lista (o item do menu, o lápis, F2 e Xadrez ▸ Renomear partida…) ficam desligados para ela e dizem por quê ("O tabuleiro mostra uma posição ilegal: corrija-a antes no editor de diagramas."). Antes, o que se digitava ia para uma partida escondida e se perdia ao fechar o app. |
