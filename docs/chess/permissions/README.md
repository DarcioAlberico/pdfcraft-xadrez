# Permissões de terceiros

Esta pasta guarda as permissões por escrito do código de terceiros usado no projeto (ver
[`../provenance.md`](../provenance.md), §3). Ela fecha a cadeia de autoria antes de qualquer
**publicação**. Não bloqueia o desenvolvimento no repositório privado.

| Terceiro | Permissão declarada | Cópia por escrito | Cobre distribuição sob `MIT OR Apache-2.0`? |
|---|---|---|---|
| PDFimport (Wagner Escorcio) | sim, pelo titular em 2026-10-15 | **pendente** | **a confirmar** |

## Como completar o PDFimport

1. O autor confirma o texto de [`pdfimport-termo-MODELO.md`](pdfimport-termo-MODELO.md) por escrito: assinado, num e-mail ou numa mensagem que fique guardada.
2. A cópia vai para esta pasta como `pdfimport-termo-AAAA-MM-DD.<pdf|eml|txt|png>`, sem editar o original.
3. Atualize a tabela acima e a última pendência de `provenance.md`.

**Se a permissão cobrir só o uso:** os trechos derivados do PDFimport continuam utilizáveis no
repositório privado. São estes trechos de `font_catalog`, `vector_detect` e `textlayer`, hoje em
`chessvision::detect::font`, `detect::vector` e `chess_text`. Antes de publicar, eles precisam ser
re-derivados da ISO 32000.

O modelo é só um rascunho para o autor. Ele **não** vale como permissão.
