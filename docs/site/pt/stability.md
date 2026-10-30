# Estabilidade

Este documento diz o que você pode usar como base na série 1.x e quando uma mudança no Meada exige uma versão maior. Ele vale a partir da 1.0: a série 0.x não carrega promessa de estabilidade, e qualquer item daqui pode mudar entre versões 0.x.

O Meada segue o [versionamento semântico](https://semver.org/lang/pt-BR/). Na série 1.x, uma mudança que quebre um item congelado só entra em uma versão maior (2.0). Uma mudança aditiva entra em uma versão menor (1.x).

Este documento descreve o build padrão (`cargo install meada`). Os [módulos de nuvem](cloud.md) só existem em builds com a feature correspondente e só acrescentam à superfície; nenhum deles muda o que está listado aqui.

## O que está congelado

### Ferramentas MCP

Três ferramentas, sempre com estes nomes. Nenhuma escreve.

| Ferramenta | Parâmetro | Tipo | Padrão | Significado |
| --- | --- | --- | --- | --- |
| `search` | `query` | texto, obrigatório | | Nome a procurar, sem distinguir maiúsculas. |
| `search` | `kind` | texto, opcional | todos | Restringe a um tipo (lista abaixo). |
| `search` | `budget` | inteiro sem sinal de 32 bits, opcional | 2.000 | Orçamento de tokens da resposta. |
| `context` | `symbol` | texto, obrigatório | | Nome do símbolo, `<caminho>::<nome>` ou identidade completa; um recurso (`resource::<tipo>::<id>`) ou um canal (`channel::<broker>::<nome>`). |
| `context` | `budget` | inteiro sem sinal de 32 bits, opcional | 2.000 | Orçamento de tokens da resposta. |
| `trace` | `from` | texto, opcional | | Início: um símbolo, uma rota (`GET /users/{}` ou seu contrato), um call site ou um processo (`process::<identidade do ponto de entrada>`, só `down`). |
| `trace` | `changes` | texto, opcional | | `uncommitted`: parte dos símbolos que o diff da árvore de trabalho toca. É o único valor aceito. |
| `trace` | `direction` | texto, opcional | `down` | `down` (o que ele alcança) ou `up` (o que o alcança). |
| `trace` | `budget` | inteiro sem sinal de 32 bits, opcional | 2.000 | Orçamento de tokens da resposta. |

- `trace` pede exatamente um entre `from` e `changes`.
- O orçamento padrão de 2.000 tokens é parte da superfície. Um token é uma estimativa: um token por quatro bytes de saída em UTF-8.
- Os tipos de `search`: `symbol`, `contract`, `route`, `resource`, `channel`, `document`, `package`, `drift`, `community`, `process`, `duplicate`, `ambiguous` e `unresolved`.

### Gramática da saída

A saída das três ferramentas é texto em linhas, e sua gramática é especificada no documento de referência da gramática de saída (em inglês, como a superfície MCP, [ADR 0008](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0008-mcp-surface-and-output-languages.md)). Estão congelados:

- cada tipo de linha de cada ferramenta e o significado de cada um dos seus campos;
- a notação de link `→[nível: motivo]`, com os níveis `exact`, `declared`, `inferred`, `decided` e `ambiguous`;
- os identificadores de contrato, cada um começando pelo seu tipo de contrato: `http::`, `channel::`, `package::`, `resource::`;
- o rodapé de `trace` (`chain: <nível mais fraco> · <n> hops · <n> repos · <n> tokens`);
- a nota que encerra uma resposta cortada pelo orçamento;
- a estimativa de tokens.

Quem lê a saída deve pular os tipos de linha e os campos que não conhece: é assim que uma versão menor acrescenta algo sem quebrar ninguém.

### Linha de comando

Comandos e flags do build padrão:

| Comando | Flags |
| --- | --- |
| `meada index` | `--workspace <manifesto>` |
| `meada status` | `--workspace <manifesto>` |
| `meada export` | `--workspace <manifesto>`, `--repo <identidade>` |
| `meada mcp` | `--listen <endereço>`, `--workspace <manifesto>` |
| `meada decide` | `--item <id>`, `--dry-run`, `--list`, `--prune`, `--decider`, `--show-request`, `--min-p <p>` |

Valem também `meada help`, `--help` e `--version` (`-h`, `-V`).

- `--workspace`, `--repo` e `--listen` são do [modo servidor](server.md). Em `meada export`, `--workspace` e `--repo` vão juntos; em `meada mcp`, `--listen` e `--workspace` vão juntos, e `--listen` serve MCP por HTTP streamable, um endpoint por identidade de repositório.
- `meada decide`: `--prune` não se combina com `--item`, `--dry-run` nem `--list`; `--show-request` e `--min-p` só valem com `--decider`; `--min-p` vai de 0 a 1 e seu padrão é 0,5.
- As variáveis de ambiente do decisor: `MEADA_DECIDER_PROTOCOL` (`decisions` ou `chat`, padrão `decisions`); `MEADA_DECIDER_URL` (padrão `https://openrouter.ai` com `decisions`, obrigatória com `chat`); `MEADA_DECIDER_KEY` (obrigatória com `decisions`, opcional com `chat`); `MEADA_DECIDER_MODEL` (padrão `typesafe/jev-1.14` com `decisions`, obrigatório com `chat`). Veja [Decisões](decisions.md).

### `meada.toml`

Todas as tabelas e chaves. Uma chave desconhecida é erro, com o arquivo, a linha e a chave.

| Tabela | Chave | Tipo | Observação |
| --- | --- | --- | --- |
| `[[peer]]` | `repo` | texto, obrigatório | Identidade do repositório par. |
| `[[peer]]` | `path` | texto, opcional | Dica de onde o clone está, relativa à raiz. |
| `[[peer]]` | `base_url_env` | lista de textos, opcional | Variáveis de onde este repositório lê a URL base do par. |
| `[serve]` | `hosts` | lista de textos, opcional | Hosts em que o repositório responde. |
| `[serve]` | `prefix` | texto, opcional | O que o gateway acrescenta antes dos caminhos. |
| `[serve]` | `strip` | texto, opcional | O que o gateway tira do início dos caminhos. |
| `[serve]` | `documents` | lista de textos, opcional | Documentos OpenAPI ou AsyncAPI além dos achados pelo nome, relativos à raiz. |
| `[[resource]]` | `kind` | texto, obrigatório | Identificador em minúsculas (letras, dígitos e hifens, começando por letra). |
| `[[resource]]` | `id` | texto, obrigatório | Não vazio. |
| `[[resource]]` | `role` | texto, obrigatório | `read`, `write` ou `read-write`. |
| `[[resource]]` | `via` | lista de textos, opcional | Símbolos que fazem o acesso, como `<caminho>::<nome qualificado>`. |

Veja a [configuração](configuration.md).

### `meada.decisions.toml`

Uma tabela, `[[decision]]`, com estas chaves. Uma chave desconhecida é erro.

| Chave | Tipo | Observação |
| --- | --- | --- |
| `kind` | texto, obrigatório | `ambiguous-link` ou `dynamic-url`. |
| `call` | texto, obrigatório | O call site do item, como `meada export` o nomeia. |
| `target` | texto, obrigatório | `<identidade do repositório provedor>::<identificador do contrato>`. |
| `by` | texto, obrigatório | `human` ou o id do modelo. |
| `p` | número, de 0 a 1 | Obrigatório quando `by` é um modelo; proibido quando é `human`. |

Cada `call` aparece em no máximo uma decisão; repeti-lo é erro.

### `meada.workspace.toml`

O manifesto de workspace do [modo servidor](server.md), lido pelo build padrão:

| Tabela | Chave | Tipo | Observação |
| --- | --- | --- | --- |
| (raiz) | `index_dir` | texto, obrigatório | Onde ficam os índices, `<index_dir>/<identidade>/index.db`. |
| `[workspace]` | `source` | texto, opcional | `file` (padrão) ou `store`; `store` exige um build com a feature `generation-pointer`, e o build padrão o recusa com um erro. |
| `[[repository]]` | `repo` | texto, obrigatório | Identidade do repositório. |
| `[[repository]]` | `path` | texto, obrigatório | Onde está o mirror. |
| `[[repository]]` | `owner` | texto, opcional | Grupo OIDC responsável; metadado da entrada. |
| `[[repository]]` | `added_by` | texto, opcional | Login do GitHub que acrescentou a entrada; metadado. |
| `[repository.serve]` | `hosts`, `prefix`, `strip`, `documents` | | Como em `[serve]` de `meada.toml`. |
| `[[repository.resource]]` | `kind`, `id`, `role`, `via` | | Como em `[[resource]]` de `meada.toml`. |
| `[[repository.peer]]` | `repo`, `base_url_env` | | Como em `[[peer]]`, sem `path`. |

As seções dos módulos de nuvem (`[valkey]`, `[snapshot]`, `[oidc]` e as demais) só existem em builds com a feature correspondente.

### Formato do `meada export`

O export de um repositório é JSON indentado e terminado por uma quebra de linha, com toda lista ordenada: os mesmos arquivos dão bytes idênticos. Uma lista vazia do nível superior some do documento, exceto `files`, `symbols`, `calls`, `providers`, `consumers` e `links`, que sempre aparecem; as listas aninhadas (`members`, `steps`, `contracts`, `fields`) sempre são escritas. O documento não traz um campo de versão do formato: o formato só cresce por acréscimos, e quem lê o export deve ignorar os campos que não conhece.

| Campo | Conteúdo |
| --- | --- |
| `repository` | Identidade do repositório. |
| `files` | `path`, `hash`. |
| `symbols` | `id`, `span` (`path`, `start`, `end`), `hash`. |
| `calls` | `id`, `caller`, `target`, `level`, `candidates`, `span`, `narrowed`, `decorates`. |
| `providers` | `contract`, `site`, `gateway`, `shape`. |
| `consumers` | `contract`, `site`, `via`, `shape`. |
| `bindings` | `contract`, `exchange`, `pattern`, `site`. |
| `documents` | `path`, `format` (`openapi` ou `asyncapi`), `version`, `contracts`. |
| `publishes` | `exchange`, `key`, `site`. |
| `packages` | `name`, `version`, `repository`, `directory`, `ecosystem`. |
| `dependencies` | `manifest`, `field`, `name`, `alias`, `range`. |
| `imports` | `site`, `specifier`, `name`, `level`, `how`, `target`. |
| `communities` | `name`, `members`. |
| `links` | `consumer`, `provider`, `contract`, `level`, `reason`, `by`, `p`, `drift` (`missing`, `unchecked`). |
| `processes` | `entry_point`, `kind` (`route`, `export`, `channel` ou `program`), `label`, `steps`. |
| `stale_decisions` | `call`, `target`, `cause`. |

Os campos de `shape` (`source`, `fields`, `open`, `scalars`, `declared`, `optional`, `guessed`) e de cada passo de `steps` (`depth`, `repository`, `level`, `via`, `node`, `reason`, `mark`, `candidates`, `contract`) fazem parte do formato, e os valores abaixo também:

- `level`: `exact`, `declared`, `inferred`, `decided` e `ambiguous`;
- `shape.source`: um objeto com uma chave, `document`, `zod`, `literals` ou `reads`, cujo valor é o caminho do arquivo;
- `drift.missing`: cada caminho com seu nível; `drift.unchecked`: cada caminho com o caminho aberto que o cobre;
- `imports[].how`: `exports`, `types`, `module`, `main`, `path`, `outDir`, `moduleName`, `topLevel`, `crateName`, `modulePath`, `package` e `namespace`;
- `steps[].via`: `entry`, `call` e `link`;
- `steps[].mark`: `cycle`, `seen`, `ambiguous`, `unresolved`, `no-provider`, `produces`, `consumes` e `depth-limit`;
- `stale_decisions[].cause`: `call-gone`, `item-not-open`, `target-gone` e `not-candidate`.

## O que é uma quebra

Depois da 1.0, uma quebra em qualquer item congelado exige uma versão maior. Quebrar é:

- remover ou renomear um item (ferramenta, parâmetro, comando, flag, tabela, chave, campo do export);
- mudar o tipo, o padrão ou o significado de um parâmetro, inclusive o orçamento padrão de 2.000 tokens;
- mudar a gramática ou o significado de um tipo de linha, de um campo ou de uma chave que já existe;
- tornar inválida uma configuração que era válida.

## O que é uma mudança menor

Estas mudanças entram em uma versão menor:

- acrescentar um parâmetro opcional;
- acrescentar um tipo de `search`;
- acrescentar um tipo de linha à saída;
- acrescentar um campo ao export;
- acrescentar uma chave de configuração;
- acrescentar um tipo de contrato;
- acrescentar um language pack.

O [modo servidor](server.md) é um exemplo: `--workspace`, `--repo` e `--listen`, o `meada.workspace.toml` e a linha `index behind the mirror: run meada index --workspace` do `context` só acrescentam à superfície, e nada que já existia muda por causa deles.

## O que não está congelado

- A redação das mensagens da linha de comando, em qualquer idioma.
- O texto das descrições das ferramentas MCP, que continua dentro do orçamento de 1.000 tokens da lista de ferramentas.
- O arquivo de índice e a organização de `.meada/`, reconstruídos quando o esquema muda ([ADR 0004](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0004-index-storage-and-freshness.md)).
- Quais links o Meada encontra para um repositório. Melhorar a resolução é uma correção, não uma quebra: um link novo, um nível mais forte ou um falso positivo a menos podem chegar em qualquer versão.

## O portão de revisão

Toda mudança na superfície congelada aparece como diff em revisão, por dois mecanismos:

- os **goldens da superfície**: a lista de ferramentas MCP (nomes, esquemas dos parâmetros e descrições, com a verificação do limite de 1.000 tokens) e a ajuda em inglês de cada comando da CLI;
- a **verificação de gramática**: os goldens das respostas renderizadas são conferidos contra a gramática de referência, de modo que todo tipo de linha que aparece neles esteja documentado.

Quem abre um PR que altera um desses goldens, ou o esquema de uma configuração ou o formato do export, diz no PR se a mudança é uma quebra ou uma mudança menor, pelas listas acima. Uma quebra só entra no próximo release maior.
