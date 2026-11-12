# Notas para o PdfCraft original (upstream)

**Status:** uma correção em área do upstream (`crates/cos`) feita no fork, à espera da decisão D24 do `plano-analise-9.md` (levar ou não ao `storytold/pdfcraft` na próxima sincronização). Nada aqui foi enviado ao upstream.

**Histórico de revisões:** 2026-11-11 — primeira versão, com o `/ID` dos documentos novos (plano A9, G1).

Cada nota diz o defeito, a correção no fork, o teste e o que o upstream veria de diferente. O fork não abre PR no upstream (o `upstream` tem push desativado, `blueprint.md` D3); a nota é o material para quem decidir levar.

## `/ID` dos documentos criados do zero (plano A9, G1; achado A4-03)

- **Defeito.** `cos::writer::ensure_id` fazia o hash de uma amostra (1 byte a cada `len/4096`, mais os últimos 64) dos bytes do **arquivo de origem**. Um documento montado do zero (`Document::new_empty`: extrair, dividir, combinar) não tem bytes de origem, e o hash com a semente fixa de `SaveOptions::default()` é uma constante: todo PDF que o PdfCraft cria sai com `/ID [<9BA0D5AAD0617164A1D073B0D8C7DCD9> …]`. Leitores e programas que usam o `/ID[0]` como identidade tomam arquivos diferentes por um só.
- **Correção no fork.** O `/ID` novo é um digest de 128 bits de **todos** os bytes escritos antes da seção de referência cruzada (no salvamento incremental, o original e a revisão nova), com a semente; mesma entrada → mesmo `/ID`. Linear, sem dependência nova, não criptográfico (a ISO 32000-2 §14.4 pede um identificador único na prática). Um arquivo que já tem `/ID` o mantém, como antes.
- **Arquivos:** `crates/cos/src/writer.rs` (`ensure_id`, `id_digest`). Teste: `crates/cos/tests/writer.rs`, `new_documents_get_an_identifier_from_the_bytes_written` (páginas diferentes → `/ID` diferentes; mesmos bytes → mesmo `/ID`; um byte diferente no meio de 200 KB → outro `/ID`).
- **O que muda para o upstream.** Os PDFs novos deixam de compartilhar o `/ID`; os bytes de saída de todo salvamento sem `/ID` mudam (testes que fixem bytes de saída teriam de ser refeitos — no fork, nenhum fixava). Os arquivos já gravados com a constante continuam com ela (o `/ID[0]` é permanente); o fork os trata como sem `/ID` só nas próprias lojas de xadrez (`DocKeys::KNOWN_CONSTANT_ID`), o que não é parte do upstream.

## Pendentes da D24 (não corrigidos aqui)

- O `/ID[1]` nunca muda numa regravação; a ISO 32000-2 §14.4 pede que mude a cada atualização.
- O "Automático" do Tamanho da interface ignora o Tamanho do texto do Windows (`apps/pdfcraft/src/text_scale.rs` lê só o GNOME; achado R5-10).
