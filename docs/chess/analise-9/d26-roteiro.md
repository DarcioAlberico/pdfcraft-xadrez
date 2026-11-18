# D26 — roteiro do NVDA para o painel de xadrez

**Status:** rascunho do passo A9-I11a (2026-11-12) · roda pela primeira vez no fim do X3 (onda 3), com o NVDA instalado (D26) · a coluna Xadrez (abas, mestre-detalhe, foco) acrescentada pelo A9-I11b (2026-11-15, §4) · o X3 completa a parte do EPUB.

Cada passo diz o que fazer só com o teclado e o que o NVDA deve dizer (o texto esperado, em pt-BR, com o app em português). Um passo que o NVDA diz de outro jeito vira um achado com o texto ouvido; um que não diz nada é falha de 4.1.2 ou 4.1.3. As frases entre aspas são as do app; o NVDA acrescenta o papel ("botão", "caixa de diálogo", "item de menu") na língua dele.

**Preparação.** NVDA com a voz em português, modo de foco (NVDA+Espaço até ouvir "modo de foco"). O livro sintético da jornada de ouro (`ui-egui/tests/chess_golden.rs`) ou um PDF com um diagrama em fonte e um tabuleiro desenhado (o `boards()` de `ui-egui/tests/chess.rs`). Sem modelo de peças: o tabuleiro desenhado fica "a revisar". Janela de 1366 × 768, Tamanho da interface 100 %, depois 200 %.

## 1. O painel e os seus menus

| # | Tecla | O NVDA diz |
|---|---|---|
| 1.1 | Abrir o painel (Ferramentas › Xadrez › Painel de xadrez) e Tab até o cabeçalho | "Detectar diagramas, botão" |
| 1.2 | Tab | "Onde detectar, botão" |
| 1.3 | Enter | "menu"; depois, com ↓, "No livro inteiro (1 página), item de menu", "Nesta página, item de menu" |
| 1.4 | Enter em "Nesta página" | o menu fecha; o foco volta ao botão; a detecção começa |
| 1.5 | Tab até "Exportar livro…" e Enter | "menu"; "PGN — partidas, item de menu" … "EPUB — livro digital, item de menu" |
| 1.6 | ↓ até "Diagramas como partidas PGN" e → | "Só os que não estão numa partida, item de menu de opção, marcado" |
| 1.7 | Esc, Esc | os menus fecham; o foco volta a "Exportar livro…" |

## 2. Perguntas (diálogos modais)

| # | Tecla | O NVDA diz |
|---|---|---|
| 2.1 | Com um diagrama a revisar, Exportar livro… › PGN | "Diagramas ainda não conferidos: 1, caixa de diálogo"; depois "Revisar primeiro, botão" (o foco está nele, não em "Exportar assim mesmo") |
| 2.2 | Tab, Tab, Tab | só os botões do diálogo: "Exportar assim mesmo, botão", "Revisar primeiro, botão"; nenhum controle do fundo |
| 2.3 | Esc | o diálogo fecha sem gravar; o foco volta a "Exportar livro…" |
| 2.4 | Tabuleiro › Analisar, sem motor instalado | "Nenhum motor de análise instalado, caixa de diálogo"; "Escolher motor…, botão" |
| 2.5 | Editar um diagrama, mudar uma casa, Detectar diagramas | "Guardar a correção do diagrama 2 (pág. 1)?, caixa de diálogo"; "Confirmar correção, botão" |
| 2.6 | ⋯ › Redesenhar no PDF… (primeira vez) | "Redesenhar no PDF?, caixa de diálogo"; "Cancelar, botão" (o PDF fica como está) |
| 2.7 | Sair com partidas que o salvamento automático não guarda | "Partidas de xadrez não salvas, caixa de diálogo"; "Salvar todas as partidas em PGN…, botão" |

## 3. Controles com nome

| # | Onde | O NVDA diz |
|---|---|---|
| 3.1 | Barra do documento, Tab até a caixa da página | "Página, editar, 1" |
| 3.2 | Aba Texto, Tab até a origem do livro | "Origem do livro, caixa de combinação, Automático (página a página)" |
| 3.3 | Aba Diagrama, Tab até o número do lance | "Número do lance:, botão giratório, 1" |
| 3.4 | Tabuleiro, Partida, campo do 1º lance | o rótulo do campo e o número |
| 3.5 | Prévia da captura | "Inserir em, caixa de combinação …"; "Letras das peças, caixa de combinação …" |

## 4. A coluna Xadrez: abas, mestre-detalhe e foco (A9-I11b)

Janela de 1366 × 768: abrir a coluna recolhe o painel Ferramentas (uma vez por sessão).

| # | Tecla | O NVDA diz |
|---|---|---|
| 4.1 | Com o teclado num controle do painel Ferramentas, abrir a coluna sem tirá-lo de lá (o comando **Painel de xadrez** por um atalho) | "Painel Ferramentas recolhido"; o foco vai à aba mostrada: "Diagramas, aba, selecionado, 1 de 3" (nunca a um controle que sumiu) |
| 4.2 | → (ou Tab) | "Texto, aba, 2 de 3"; → "Tabuleiro, aba, 3 de 3"; ← volta. Enter ou Espaço numa aba a mostra |
| 4.3 | Na aba Diagramas, Tab | "Diagramas, painel de abas" ao entrar no conteúdo; depois "Detectar diagramas, botão", "Onde detectar, botão" … e as linhas da lista: "Diagrama 2 · A revisar" |
| 4.4 | Enter na linha "Diagrama 2" | o editor toma o lugar da lista; o foco vai ao tabuleiro: "Tabuleiro do diagrama, pág. 1, diagrama 2, grade" e a casa do cursor |
| 4.5 | Tab pelo editor até o fim | cada controle com nome, nenhum fora da tela; "Voltar para Diagramas, botão" está entre eles |
| 4.6 | Esc (com o foco no tabuleiro) | a lista volta; o foco vai à linha de onde o editor saiu: "Diagrama 2 · A revisar" |
| 4.7 | Aba Texto, Tab pelos lances até depois da dobra | cada lance dito em palavras ("3. bispo b5"); a aba rola para mostrar o lance com o foco |
| 4.8 | Aba Tabuleiro, Tab pelos controles | cada um com nome; nenhum fica escondido sob o pé da coluna |

A Lupa do Windows, com "seguir o foco do teclado", mostra em cada passo o anel de 2 pt na cor de destaque em volta do controle (a casa do cursor, no tabuleiro do editor).

## 5. O que só o NVDA confere

- Com o diálogo aberto, a navegação do modo de navegação (NVDA+Espaço) não alcança os 131 botões do fundo (o nó do diálogo é `modal`).
- O anel de foco aparece em cada controle alcançado por Tab, com a janela em uso (A6-20); a Lupa do Windows, com "seguir o foco do teclado", o acompanha.
- Os menus fechados por Enter não deixam o NVDA num item que sumiu.

## 6. EPUB exportado (o X3 completa)

Abrir o EPUB exportado no Thorium com o NVDA: título do livro, a navegação por títulos (H), cada diagrama com a descrição ("Diagrama 1: brancas jogam; …"), os sinais lidos pelo nome (`aria-label`), o idioma do texto. O X3 escreve o texto esperado por passo.
