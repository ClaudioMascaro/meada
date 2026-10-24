# Configuração

Cada repositório pode ter um `meada.toml` na raiz, versionado no git, dizendo com quem ele
conversa. Quem consome uma rota nomeia o par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sem esquema nem .git
base_url_env = ["API_URL"]               # variável de onde o cliente lê a URL base
# path = "../api"                        # opcional: dica de onde o clone está
```

Quem serve a rota declara os hosts em que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: o gateway põe isto na frente dos caminhos
strip = "/v1"                            # opcional: e tira isto do começo deles
documents = ["docs/api-contract.yaml"]    # opcional: documentos adicionais, até ignorados pelo git
```

Com `prefix` e `strip`, uma chamada a `/shop/orders` chega à rota `/v1/orders` do código, no
nível `inferred` e com o motivo `gateway prefix /shop, gateway strip /v1`. Um segmento de versão
só sai quando você o escreve em `strip`, e quem chama o caminho do código continua ligando como
antes.

Um recurso compartilhado que nenhum código nomeia, como uma tabela ou um bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador em minúsculas
id = "orders"
role = "write"                           # read, write ou read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: os símbolos que fazem o acesso
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`.
Sem `via`, ou se nenhum `via` corresponde a um símbolo, o lado é o repositório inteiro.

## Referência completa de `meada.toml`

Só existem as três tabelas abaixo. Sem arquivo, `peer` e `resource` são listas vazias e
`serve` está ausente. `[[peer]]` e `[[resource]]` aceitam várias entradas; `[serve]` é única.
Uma chave desconhecida, tipo errado ou valor inválido dá erro com arquivo, linha e chave.
Configuração de [modo servidor](server.md) e de [decisores](decisions.md) tem outros canais;
não acrescente tabelas de bootstrap ou credenciais ao `meada.toml` local.

| Tabela e chave | Tipo e padrão | Significado e validação |
| --- | --- | --- |
| `peer.repo` | string obrigatória | Identidade do par já normalizada, por exemplo `example.com/acme/api`; deve corresponder ao `origin` do clone encontrado. |
| `peer.path` | string opcional, ausente | Dica de onde está o clone, relativa à raiz deste repositório; `../api` é válido. Não faz clone nem fetch. |
| `peer.base_url_env` | lista de strings, `[]` | Nomes de variáveis que o código usa para a URL base do par; os valores do ambiente nunca são lidos. |
| `serve.hosts` | lista de strings, `[]` | Hosts em que este repositório serve HTTP, comparados em minúsculas. Não há uma validação adicional de sintaxe de hostname nesta chave. |
| `serve.prefix` | string opcional, ausente | Prefixo que o gateway adiciona; deve começar com `/`, é normalizado e perde a `/` final. `/` sozinho não reescreve. |
| `serve.strip` | string opcional, ausente | Prefixo que o gateway remove do caminho do código, só em segmentos completos; mesma validação de `prefix`. |
| `serve.documents` | lista de strings, `[]` | Arquivos adicionais de OpenAPI/AsyncAPI, relativos à raiz; rejeita caminhos começando com `/` ou contendo segmento `..`; remove segmentos `.` e `/` repetidas. |
| `resource.kind` | string obrigatória | Identificador ASCII que começa com letra minúscula e continua com letras minúsculas, dígitos ou `-`, como `postgres` ou `s3-bucket`. |
| `resource.id` | string obrigatória | Identificador do recurso; a string vazia é inválida. |
| `resource.role` | string obrigatória | Exatamente `read`, `write` ou `read-write`. |
| `resource.via` | lista de strings, `[]` | Símbolos locais no formato `caminho::nome-qualificado`; os que não correspondem a símbolos aparecem no status. Sem correspondência, o lado é o repositório inteiro. |

Meada identifica o repositório pelo remoto git: normaliza host/caminho para minúsculas e
remove esquema, usuário, porta e sufixo `.git`. `peer.repo` já deve estar nessa forma, pois
seu texto não é normalizado silenciosamente. Pares transitivos e clones irmãos que nomeiam
um repositório do workspace também podem entrar nele. Meada usa os clones existentes e
confere sua identidade; a dica `path` é necessária para localizar um par sem remoto.
Cada repositório continua com seu próprio índice.

Mesmo `[serve]` vazio habilita documentos como fontes de provedores. A descoberta automática
reconhece `openapi.yaml`, `openapi.yml`, `openapi.json`, os equivalentes `asyncapi` e nomes
terminados em `.openapi.<ext>` ou `.asyncapi.<ext>`, com essas três extensões. Ela respeita o
walk normal e suas regras de ignore; `documents` acrescenta arquivos ignorados ou de outros
nomes. Sem `[serve]`, documentos não dão provedores. Arquivos ausentes ou ilegíveis são
relatados em `meada status`; não dão provedores adivinhados.

Os documentos precisam ser arquivos regulares dentro do repositório, sem atravessar links
simbólicos. Essa contenção é dos arquivos de entrada; a dica de clone `peer.path = "../api"`
continua permitida. Veja [Capacidades e limites](capabilities.md) para leitura de documentos
e ligação de contratos.
