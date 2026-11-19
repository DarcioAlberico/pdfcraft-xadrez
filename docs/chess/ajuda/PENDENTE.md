# Ajuda do xadrez — o que ficou velho

A ajuda em PDF (`Ajuda - Ferramentas de Xadrez.pdf`) descreve o build que a capa dela nomeia: `7496134 2026-11-19`, refeita no passo A9-E1 (e no A9-E1c, com a Biblioteca). A fonte é [`ajuda.html`](ajuda.html), nesta pasta; [`README.md`](README.md) diz como refazer o PDF e onde ficam as figuras e os geradores. O texto da ajuda anterior (31/10, build `cc2d484c`) fica em [`texto-2026-10-31.txt`](texto-2026-10-31.txt).

Cada passo do plano A9 (`../plano-analise-9.md`) que muda algo visível corrige `ajuda.html` no mesmo PR ou acrescenta aqui uma linha com o capítulo que ficou velho (os capítulos são os da ajuda refeita). A próxima revisão da ajuda refaz as capturas a partir desta lista e a esvazia.

| Passo | PR | Capítulo da ajuda | O que mudou |
|---|---|---|---|
| A9-F7 | (ainda não no `main`) | 10 — A lista de lances; 13 — Sinais e comentários (Diagrama aqui); 17 — Preferências e aparência (a seção Xadrez); 18 — Exportar o livro (o diálogo) | Os comandos de um comentário (`[%clk]`, `[%eval]`, `[%emt]`, `[%evp]`, `[%tqu]`, `[#]` do ChessBase) não aparecem mais: um lance de uma partida do lichess não fica "comentado", o comentário grande mostra só as palavras, e o nome falado não diz "com comentário". Preferência nova, desligada: **Mostrar o relógio e a avaliação que o PGN dá a cada lance** (dois selos depois do lance). `$201` (Scid, ChessDB) e o `{[#]}` do ChessBase são marcas de diagrama. O diálogo de exportação ganhou **PGN para** (nos PGNs de partidas e de exercícios): **PGN padrão**, **Lichess** ou **ChessBase**. Nenhum rótulo que a ajuda cita mudou. |
