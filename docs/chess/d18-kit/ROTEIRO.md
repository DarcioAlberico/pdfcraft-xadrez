# Sessão D18 no ChessBase 26 — roteiro de 15 minutos

**Para quê.** A decisão D18 do plano A9 (`../plano-analise-9.md`, §7) pede uma sessão curta no ChessBase 26 que responda quatro perguntas antes dos passos F7 e X1b: (1) como o CB lê e grava PGN em UTF-8 com e sem BOM e em Windows-1252; (2) que código o CB grava para uma marca de diagrama, e se lê os do Scid e do ChessPad; (3) se o CB lê `$37`/`$23` e os outros NAGs "das pretas"; (4) como o CB grava um exercício (`%tqu`, decisão D20). Os arquivos desta pasta são sintéticos, escritos para o teste: nenhum vem de livro ou banco de dados.

**Antes de começar (1 min).** Copie esta pasta para fora do repositório (por exemplo `C:\Temp\d18`). Abra `respostas.csv` no Excel ou no Bloco de Notas ao lado do CB: cada passo abaixo tem a sua linha. Escreva o que o CB mostra, com as palavras dele; "não sei" também é resposta.

| Arquivo | O que tem |
|---|---|
| `1-utf8.pgn` | duas partidas com acentos (Réti, José Raúl, São Paulo, Göteborg, Müller, Peña, Ñuño) e comentários em português, em UTF-8 sem BOM |
| `2-utf8-com-bom.pgn` | o mesmo texto em UTF-8 com BOM (`EF BB BF` no início) |
| `3-windows-1252.pgn` | o mesmo texto em Windows-1252 (o ISO 8859-1 do padrão PGN, mais as aspas e o travessão do Windows) |
| `4-nags-das-pretas.pgn` | uma partida com `$36`/`$37` (iniciativa), `$22`/`$23` (zugzwang) e `$132`/`$133` (contrajogo) |
| `5-marcas-de-diagrama.pgn` | uma partida com `$201` (Scid, ChessDB), `$220` e `$221` (ChessPad) |
| `6-exercicio.pgn` | dois exercícios a partir de uma FEN: um sem nada, outro com um `%tqu` escrito como **suposição** do formato do CB |

## 1. Codificação (5 min)

1. **A1.** Abra `1-utf8.pgn` no CB (Arquivo ▸ Abrir, tipo PGN). Os nomes das duas partidas na lista aparecem certos (`Réti`, `José Raúl`, `Müller`, `Peña`, `Ñuño`)? E o local `São Paulo` e os comentários (`peça`, `ênfase`, `ótimo`)?
2. **A2.** O mesmo com `2-utf8-com-bom.pgn`. Aparece uma partida a mais, vazia, ou a primeira perde o `Event`?
3. **A3.** O mesmo com `3-windows-1252.pgn`.
4. **A4.** Com `1-utf8.pgn` aberto, salve as duas partidas num PGN novo (`saida-cb.pgn`; se o CB perguntar a codificação, anote as opções que ele dá e escolha a padrão). Depois, no PowerShell: `Format-Hex .\saida-cb.pgn | Select-Object -First 3`. A primeira linha começa com `EF BB BF` (BOM)? O `é` de `Réti` aparece como `C3 A9` (UTF-8) ou `E9` (Windows-1252)?

## 2. Marcas de diagrama (3 min)

5. **B1.** Abra `5-marcas-de-diagrama.pgn`. O CB mostra um diagrama depois de `3...Bb4`, de `7...Nc6` e de `10...Qc7`? Em quais?
6. **B2.** Numa partida qualquer, ponha um diagrama depois de um lance (no CB, o símbolo de diagrama dos comentários) e salve como PGN (`diagrama-cb.pgn`). Abra o arquivo no Bloco de Notas: que código aparece depois do lance (`$220`? `$201`? outro)? Copie a linha.

## 3. NAGs das pretas (2 min)

7. **C1.** Abra `4-nags-das-pretas.pgn`. Os símbolos depois de `7...d6` (`$37`), `9...Na5` (`$23`) e `10...c5` (`$132`) aparecem? Iguais aos das brancas em `5.O-O` (`$36`), `9.h3` (`$22`) e `11.d4` (`$133`)?
8. **C2.** Salve essa partida como PGN (`nags-cb.pgn`) e procure no Bloco de Notas: o CB grava `$37` e `$23`, ou troca pelos das brancas (`$36`, `$22`)?

## 4. Exercício (`%tqu`, decisão D20) (4 min)

9. **D1.** Abra `6-exercicio.pgn`. O segundo exercício traz um `%tqu` escrito como **suposição**. O CB o mostra como pergunta de treino, como comentário comum, ou o esconde?
10. **D2.** No primeiro exercício (sem `%tqu`), crie uma pergunta de treino no CB com **duas** respostas certas (`Rd8#` e outra qualquer que você aceitar como certa) e uma pontuação. Salve como PGN (`exercicio-cb.pgn`) e copie para a planilha a linha com `%tqu` como ela saiu. Anote: o comando tem um campo de idioma? O lance da resposta está em SAN (`Rd8#`) ou em coordenadas (`d1d8`)? O comando fica antes do primeiro lance?
11. **D3.** Se o CB pedir um banco com o atributo "Training" (ou um nome parecido) para gravar a pergunta, anote o caminho do menu.

## Ao terminar

Salve `respostas.csv` e mande-o junto com `saida-cb.pgn`, `diagrama-cb.pgn`, `nags-cb.pgn` e `exercicio-cb.pgn` (são arquivos que o CB gravou a partir dos arquivos sintéticos desta pasta). Eles ficam fora do repositório; o passo F7 transforma o que eles mostram em testes.
