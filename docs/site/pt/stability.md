# Estabilidade em 1.x

Esta árvore declara **0.1.0** e prepara a 1.0. A série 0.x não tem promessa de estabilidade.
A política abaixo passa a valer **quando a 1.0 for publicada** e cobre as versões 1.x;
este documento não anuncia uma release nem afirma que os gates de aceitação passaram.

## Superfície congelada

Os inventários ligados abaixo fazem parte desta política. Eles registram a superfície
existente, com seus tipos, padrões, validações e significados.

| Superfície | O que fica congelado em 1.x | Inventário completo |
| --- | --- | --- |
| MCP | Nomes de `search`, `context` e `trace`, nomes/tipos/padrões dos parâmetros abaixo e gramática da saída. | [Agentes e MCP](mcp.md) e [gramática normativa de saída](https://github.com/ClaudioMascaro/meada/blob/main/docs/reference/output-grammar.md). |
| CLI | Comandos, subcomandos e flags, inclusive os disponíveis só com a feature correspondente no build. Inclui os comandos locais, `snapshot`, `github-app`, `ui`, `workspace`, ajuda e versão. | [Comandos](cli.md), com argumentos, padrões e restrições. |
| `meada.toml` local | `[[peer]]`: `repo`, `path`, `base_url_env`; `[serve]`: `hosts`, `prefix`, `strip`, `documents`; `[[resource]]`: `kind`, `id`, `role`, `via`. | [Configuração](configuration.md), cada tabela e chave com tipo, padrão e validação. |
| `meada.decisions.toml` | `[[decision]]`: `kind`, `call`, `target`, `by`, `p`, incluindo a unicidade de `call` e as regras condicionais de `p`. | [Decisões](decisions.md), com os tipos e valores aceitos. |
| `meada export` | Campos, tipos e significados do JSON canônico, inclusive registros aninhados, enums, `null`, omissões e ordenação. | [Export JSON](export.md). |

### Parâmetros MCP

| Ferramenta | Parâmetro | Tipo JSON | Padrão e significado |
| --- | --- | --- | --- |
| `search` | `query` | string obrigatória | Sem padrão; nome a procurar, sem distinção de maiúsculas/minúsculas. |
| `search` | `kind` | string ou `null`, opcional | Ausente ou `null`: símbolos e contratos de provedores/consumidores. Uma string escolhe um dos tipos de busca do [inventário MCP](mcp.md). |
| `context` | `symbol` | string obrigatória | Sem padrão; símbolo ou identidade de recurso/canal a descrever. |
| `trace` | `from` | string ou `null`, opcional | Ausente ou `null`: não escolhe um início por identidade. Aceita símbolo, rota, chamada ou `process::<identidade-do-ponto-de-entrada>`. |
| `trace` | `changes` | string ou `null`, opcional | Ausente ou `null`: não escolhe o diff. Só aceita `uncommitted`. |
| `trace` | `direction` | string ou `null`, opcional | Ausente ou `null`: `down`; também aceita `up`. Processos só aceitam `down`. |
| Todas | `budget` | inteiro de 0 a 4.294.967.295 ou `null`, opcional | Ausente ou `null`: **2.000 tokens** por resposta. |

`trace` exige exatamente um entre `from` e `changes` com valor não nulo. Modo servidor
recusa `changes: uncommitted`. Os padrões acima são o comportamento efetivo, inclusive quando
o cliente omite a propriedade ou envia `null`.

A gramática normativa é escrita em inglês, como a superfície MCP. Ela especifica os tipos
de linha, identificadores de contrato, níveis/motivos, rodapé, marcador de corte e estimativa
de tokens: um token por quatro bytes UTF-8, arredondando para cima. Leitores de 1.x devem
**ignorar tipos de linha e campos que não conhecem**, inclusive campos novos em tipos de
linha conhecidos. Leitores do export também devem ignorar campos desconhecidos.

## Quando mudar a versão

Depois da publicação da 1.0, uma quebra em qualquer item congelado exige uma versão
**major**, como 2.0. É quebra:

- remover ou renomear um item;
- mudar o tipo, o padrão ou o significado de um parâmetro;
- mudar a gramática ou o significado de um tipo de linha, campo ou chave existente;
- tornar inválida uma configuração que era válida.

São mudanças **minor**, como 1.1: acrescentar um parâmetro opcional, um tipo de `search`,
um tipo de linha, um campo do export, uma chave de configuração, um tipo de contrato ou
um pacote de linguagem. As adições preservam os itens existentes conforme as regras acima.

## O que não fica congelado

- O texto das mensagens da CLI, em qualquer idioma.
- O texto das descrições das ferramentas MCP, ainda dentro do orçamento de **1.000 tokens**
  da lista de ferramentas (descrições e schemas de parâmetros).
- O arquivo do índice e a disposição de `.meada/`; uma mudança de schema pode reconstruir
  o índice.
- Quais ligações Meada encontra para um código específico. Melhorar a resolução é uma
  correção, não uma quebra da promessa de estabilidade.

## Gate de revisão da superfície

Antes de aprovar uma mudança, compare-a com os inventários e revise os diffs destes
[golden files](https://github.com/ClaudioMascaro/meada/tree/main/tests/golden):

- [`surface.mcp-tools.json`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.mcp-tools.json): nomes, schemas e descrições das três ferramentas. [`tests/it/mcp.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/mcp.rs) verifica também o teto de 1.000 tokens e o padrão efetivo de 2.000 por ferramenta.
- [`surface.cli.default.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.default.en.txt) e [`surface.cli.cloud.en.txt`](https://github.com/ClaudioMascaro/meada/blob/main/tests/golden/surface.cli.cloud.en.txt): ajuda em inglês de todos os comandos dos builds padrão e completo. [`tests/it/cli.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/cli.rs) percorre os subcomandos.
- [`tests/it/grammar.rs`](https://github.com/ClaudioMascaro/meada/blob/main/tests/it/grammar.rs): confere os inventários da referência normativa e valida cada golden de resposta renderizada contra a gramática.

Esses checks tornam uma mudança visível para revisão; atualizar um golden não torna uma
quebra compatível com 1.x. Mudanças só na redação continuam fora do congelamento descrito
acima, mesmo quando aparecem no diff de um golden.
