# Sessão D18 — resultado (2026-11-19)

**Como foi feita.** O usuário preferiu não fazer a sessão ele mesmo e deixou o ChessBase 26 (64 bits) aberto. O agente rodou o roteiro (`ROTEIRO.md`) nele por controle da tela, com uma cópia deste kit em `C:\Temp\d18`. Os arquivos que o ChessBase gravou ficam fora do repositório (`C:\Temp\d18\saida-cb.pgn` na máquina do usuário). O passo F7 transforma o que eles mostram em testes escritos à mão. O agente desfez tudo o que acrescentou: tirou da lista "My Databases" o atalho que a sessão criou, e os arquivos do kit não foram alterados. Nada no ChessBase foi configurado.

## Respostas

| Passo | Resposta |
|---|---|
| A1 | UTF-8 sem BOM: nomes, local e comentários certos (Réti, José Raúl, Müller, Peña, Ñuño, São Paulo, peça, ênfase, ótimo, Ação). |
| A2 | UTF-8 com BOM: certos; duas partidas, nenhuma vazia a mais; o `Event` da primeira está lá. |
| A3 | Windows-1252: todo caractere acentuado vira o de substituição (U+FFFD): `R?ti`, `S?o Paulo`, `pe?a`. O CB lê PGN só como UTF-8. |
| A4a | Ao gravar PGN, o CB não pergunta a codificação. |
| A4b | Grava sempre com BOM (`EF BB BF`), até um PGN novo e vazio. |
| A4c | Grava UTF-8 (`é` = `C3 A9`). Partidas copiadas de outro PGN saem byte a byte iguais à fonte (LF). Uma partida gravada pelo próprio CB sai com CRLF e todos os lances numa linha, sem linha em branco antes do `[Event` seguinte quando a anterior terminava com LF. |
| B1 | `$201`, `$220` e `$221` não mostram diagrama; o CB os ignora. |
| B2 | O diagrama é gravado como um comentário, não como NAG: `5. Bd3 {[#]} d5`. Ao gravar, o CB apaga `$201`, `$220` e `$221`. |
| C1 | Só os códigos das brancas aparecem: `$36` (↑), `$22` (⊙) e `$132` (⇆). `$37`, `$23` e `$133` não aparecem. (No padrão PGN, `$132` é o contrajogo moderado das brancas e `$133` o das pretas; os comentários de `4-nags-das-pretas.pgn` dizem o contrário.) |
| C2 | Ao gravar, o CB apaga `$37`, `$23` e `$133`; não os troca pelos das brancas. |
| D1 | O `%tqu` suposto em `6-exercicio.pgn` é lido como pergunta de treino (o diagrama, a pergunta e o link Solution, com o lance escondido) e regravado igual. |
| D2 | Com duas respostas: `{[%tqu "Pt","Mate em 1.","","","d1d8","",10,"h2h3","",5] As brancas jogam e dão mate.} 1. Rd8# *`. O primeiro campo é o idioma (o diálogo tem as abas En, De, Fr, Es, It, Ne, Pt), depois vêm a pergunta e dois campos vazios. Cada resposta é um trio: o lance em coordenadas, o retorno e a pontuação. O comando fica antes do primeiro lance, dentro do comentário que já estava ali, com a prosa depois dele. |
| D3 | Nenhum banco com atributo de treino é pedido. A pergunta é criada em Training ▸ Training Annotation, com o cursor no início da partida, e gravada num PGN comum. |
| Outros | Ao gravar uma partida, o CB acrescenta etiquetas: `[Annotator "<nome do usuário do Windows>"]`, `[ECO]`, `[WhiteFideId "-1"]` (duas vezes; o do Black falta), `[PlyCount]`, `[GameId]` e `[EventDate "2026.??.??"]`. Acrescenta também `{[%evp …]}` antes do primeiro lance, o perfil de avaliação, em que 32767 = sem valor. Depois de um comentário, não escreve `5...` antes do lance das pretas. |
| N17 (D) | A "Blunder check" (Analysis ▸ Tactical Analysis, com "Blunder Categories") não foi observada: o CB disse "Cannot load engine", porque não tem motor configurado. Configurar um muda o programa do usuário. |

## O que muda no plano

- **F7:** lê o `{[#]}` do ChessBase como marca de diagrama, além de `$201`, `$220` e `$221`. Esconde da prosa e guarda o `[%evp …]` e o `[%tqu …]`. O leitor aceita etiquetas repetidas, partidas sem linha em branco entre elas, CRLF misturado com LF e o lance das pretas sem número depois de um comentário. O dialeto padrão continua gravando os NAGs das pretas como estão.
- **D20:** o formato do `%tqu` é conhecido. Gravar os exercícios com ele volta como passo próprio.
- **N17c:** continua fora até a "Blunder check" ser observada, o que pede um motor configurado no ChessBase.
- **X1b:** nada muda, porque o Word não estava na sessão.
