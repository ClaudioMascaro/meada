# Capacidades e limites

A árvore atual (0.1.0, em preparação para a 1.0) indexa TypeScript, JavaScript, Python,
Rust e Go. Liga repositórios por HTTP, recursos declarados, pacotes e canais de mensagens;
OpenAPI e AsyncAPI também fornecem lados de contratos. Os resultados indicam o nível de
certeza de cada ligação, sem executar a aplicação nem usar um modelo de linguagem.

O modo servidor inclui [console](console.md), [snapshots](snapshots.md), [retenção](generations.md) e [workspace gerenciado](workspaces.md), em módulos opcionais. A console tem editor, diff, playground e grafo. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) e [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) trazem exemplos de implantação.

## Linguagens e resolução

| Linguagem | Arquivos lidos |
| --- | --- |
| TypeScript e JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py` |
| Rust | `.rs` alcançados pela árvore de módulos de cada crate |
| Go | `.go` incluídos pelas regras de build do pacote |

`exact` identifica uma resolução certa pelas regras lidas; `declared` vem de configuração e
`decided` de uma decisão gravada. Na resolução de chamadas dentro de um repositório,
`inferred` e `ambiguous` são correspondências pelo nome. Em contratos, `inferred` também pode
vir de `base_url_env`, de um provedor único, de reescrita de gateway ou de um import de pacote
via `outDir`; `ambiguous` indica vários provedores que se encaixam. Um salto interno só
encontrado pelo nome não alimenta a avaliação de valores de contratos.
Uma montagem ou parte de caminho que não pode ser lida não vira um provedor adivinhado.

### TypeScript e JavaScript

Imports relativos, exports e re-exports conectam os arquivos. Para aliases, lê
`compilerOptions.baseUrl` e `paths` do `tsconfig.json` da raiz, ou `jsconfig.json` quando não
há o primeiro, com sintaxe JSONC. Não segue `extends` nem configurações de projetos aninhados.
`package.json` fornece os pacotes e suas dependências; veja [Pacotes npm](#pacotes-npm) para
entradas e mapeamento de build para fonte.

HTTP cobre `fetch`, inclusive wrappers acompanhados por chamadas `exact`, Next.js app router
e Express. Bases vindas de `process.env.API_URL`, `process.env["API_URL"]` e
`import.meta.env.API_URL` podem identificar um par por `base_url_env`. A referência à
variável também atravessa formas de string como `trim` e `replace`; seu valor nunca é lido.
Para mensagens, cobre `kafkajs`, `amqplib`, AWS SDK v3, `sqs-consumer`, `nats`, `ioredis`,
`node-redis` e jobs BullMQ, conforme a [tabela de canais](#canais-de-mensagens).

Métodos cujo receptor não pode ser resolvido ficam em correspondência pelo nome. A leitura
HTTP não segue `require` nem cadeias Express `router.route(path)`. Um handler kafkajs
passado como parâmetro de wrapper não é acompanhado. Esses recortes não se tornam ligações
certas só porque o nome de uma função coincide.

### Python

Imports partem da raiz do repositório, depois das raízes declaradas em `pyproject.toml`
(setuptools `package-dir`/`packages.find.where`, Poetry `packages.from`), ou `src` quando
nenhuma foi declarada. Classes locais usam a ordem C3 de herança; membros de uma base externa
continuam opacos. `pyproject.toml` e `requirements*.txt` também alimentam os
[pacotes Python](#pacotes-python).

HTTP cobre `requests`, `httpx` e `aiohttp`, com provedores FastAPI, Flask e Django. Clientes
atribuídos a nomes, ligados por `with`, guardados em um atributo `self` por uma chamada ou
passados por parâmetros/retornos reconhecidos podem ser acompanhados. Para mensagens, cobre
`confluent-kafka`, `kafka-python`, `pika`, `boto3`, `nats-py` e `redis-py`.

Bases reconhecem `os.environ["API_URL"]`, `os.environ.get("API_URL")` e
`os.getenv("API_URL")`, além de `environ`/`getenv` escritos sem o prefixo. O leitor reconhece
essas grafias sintaticamente: `o.getenv` após `import os as o` não é reconhecido como leitura
de ambiente. Defaults literais, `or`, f-strings e
métodos como `rstrip`, `strip` e `removesuffix` preservam a referência; outro acesso de
ambiente no fallback torna a base desconhecida. Valores de ambiente nunca são lidos.

URLs por `str.format` ou `%` não são dobradas. Receptores que são o resultado imediato de
uma chamada (`Session().get(...)`, `make().get(...)`) e atribuições como `self.http = http`
não são acompanhados. Um parâmetro que recebe clientes com bases diferentes fica sem base
conhecida. Montagens/includes não lidos não dão rotas; métodos herdados de class-based views
e `as_view()` de uma classe externa ficam de fora.

### Rust

Cada `Cargo.toml` com `[package]` define um crate, em qualquer pasta. O Meada segue seus `mod` sem executar o toolchain Rust. Lê funções, tipos, traits, módulos e métodos de `impl`. Um arquivo que nenhum `mod` alcança fica de fora.

Chamadas como `crate::a::f()`, `Tipo::new()` e `Self::f()` seguem o caminho escrito no código. A resolução também acompanha `use`, grupos, renomeações, globs, `pub use` e outros crates do workspace pelo nome.

Para `x.m()`, o Meada usa o tipo do receptor quando ele aparece em um `let`, parâmetro, campo, `self` ou retorno de função. Um `dyn Trait` ou genérico limitado por trait chega ao método do trait. Sem tipo escrito, a resolução usa o nome. Métodos ausentes dos `impl` do tipo, como os gerados por `derive` ou vindos de traits externos, não geram ligação. Atributos de rota também não ligam a funções.

A análise usa a configuração padrão de cada crate para x86_64 Linux. Itens com `#[cfg(..)]` falso e features fora do `default` ficam de fora; módulos `#[cfg(test)]` entram. Chamadas dentro de macros não são indexadas. Os contratos Rust cobrem pacotes, HTTP e canais.

HTTP cobre clientes `reqwest`/`ureq` e provedores axum, actix-web e Rocket. Bases reconhecem
`std::env::var`, `env::var` após `use std::env`, `dotenvy::var` e `env!`, inclusive formas
como `?`, `unwrap`, `expect` e `unwrap_or` com fallback sem outra leitura de ambiente.
`option_env!` e fallbacks com várias variáveis não dão uma base conhecida. O nome da variável
é lido, nunca seu valor. `Cargo.toml` fornece crates, configuração padrão e
[dependências Cargo](#pacotes-rust); não é necessário executar Cargo para indexar.
Os clientes de canais são `rdkafka`, `lapin`, `aws-sdk-sqs`, `async-nats` e `redis`.

O leitor de valores tem regras próprias para `format!`, `concat!` e `env!`, e Rocket
reconhece o registro `routes!`, sem expandir macros em geral. Clientes em campos de structs,
`static` ou `OnceLock`, URLs por `Url::parse`/`join`
e métodos de builder não reconhecidos ficam de fora.

### Go

O caminho `module` de cada `go.mod` resolve imports para pastas de pacotes, incluindo `replace` para uma pasta do repositório. Um módulo aninhado controla sua subárvore. Arquivos na mesma pasta com o mesmo `package` compartilham escopo, inclusive `_test.go`. O Meada não executa o toolchain Go.

A análise usa linux/amd64 e compilador `gc`. Arquivos `_windows.go`, com `//go:build` falso ou com cgo (`import "C"`) ficam de fora. O Meada lê funções, tipos, interfaces e métodos (`Tipo.Metodo`). Resolve chamadas no pacote e por imports (`pkg.F()`, alias ou ponto); biblioteca padrão e módulos externos continuam externos.

Uma chamada `recv.M()` é `exact` quando o código informa o tipo em parâmetro, campo, `var`, literal composto ou resultado de função. Sem isso, a resolução pelo nome é `inferred` ou `ambiguous`. Uma chamada por interface para no método da interface.

HTTP cobre `net/http` e resty como clientes; `net/http`, gin, echo, chi e gorilla/mux como
provedores. Bases reconhecem `os.Getenv` e `os.LookupEnv`, inclusive defaults nos testes
`v == ""`, `len(v) == 0`, `!ok` ou `!ok || v == ""` quando `ok` vem do mesmo `LookupEnv`.
Também acompanha `strings.TrimSuffix`, `TrimRight` e `TrimSpace`, sem ler valores do ambiente.
O manifesto é `go.mod`; não lê `go.sum`, consulta proxy ou executa o toolchain.
Os clientes de canais são `segmentio/kafka-go`, `confluent-kafka-go`, `sarama`, `amqp091-go`,
`aws-sdk-go-v2`, `nats.go` e `go-redis`. Imports de fora do workspace e um `replace` por outro
módulo não passam a fornecer código local; veja [Módulos Go](#modulos-go).

## HTTP e documentos de contrato

O Meada liga consumidores e provedores, inclusive entre linguagens e atrás de gateways:

| Linguagem | Consumidores HTTP | Provedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch` | Next.js app router, Express |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |

A ligação considera método e caminho normalizados, host literal ou o par indicado pelo nome
de variável em `base_url_env`. Uma rota de qualquer método usa `http::ANY::<caminho>`;
parâmetros escritos na rota, como `:id` ou `{id}`, viram `{}`. Uma parte desconhecida do
caminho em tempo de execução não é tratada como parâmetro conhecido.

## Recursos declarados e gateways

Um recurso que o código não nomeia, como uma tabela ou bucket, é declarado em
`[[resource]]`: `kind`, `id`, `role` (`read`, `write`, `read-write`) e `via` opcional. Por
exemplo, `api` escreve `resource::postgres::orders` e `worker` o lê. Cada escritor liga aos
leitores do mesmo tipo/id em outros repositórios, no nível `declared`. Sem símbolos `via`
correspondentes, o lado é o repositório inteiro; o status relata os nomes que não encontrou.

Em HTTP, `[serve] prefix` e `strip` descrevem o gateway. Com `prefix = "/shop"` e
`strip = "/v1"`, `/shop/orders` chega ao caminho de código `/v1/orders`; a ligação é
`inferred` e mostra a reescrita no motivo. Só segmentos inteiros de `strip` são removidos;
o caminho original do código continua ligando como antes. Veja a
[referência completa de configuração](configuration.md) para hosts, pares e defaults.

## Documentos OpenAPI e AsyncAPI

Com `[serve]` presente, mesmo vazio, documentos OpenAPI **3.0/3.1** e AsyncAPI **2.x/3.0**
podem fornecer rotas e lados de canais com shapes de resposta/payload. A descoberta por nome
e a lista `serve.documents` seguem a [configuração](configuration.md); sem `[serve]`, os
documentos não fornecem provedores. `meada status` mostra o que foi lido, ignorado ou ficou
ilegível. Referências externas não são buscadas pela rede; partes não lidas não são adivinhadas.

Uma rota documentada com o mesmo contrato da rota de código conserva seu handler. Uma rota
só documentada tem contrato e identidade próprios, mas nenhum handler cujo código percorrer.
O primeiro documento por caminho que lista a rota prevalece para sua shape; a shape de
documento tem preferência sobre zod e literais do código. Caminhos dos `servers` dão formas
de gateway com esse prefixo e `serve.strip`; sem caminho de servidor, vale a forma de `[serve]`.
Partes abertas do schema ficam não conferidas na análise de drift.

## Canais de mensagens

Cada produtor se liga ao handler dos consumidores, inclusive no mesmo repositório.

| Canal | TypeScript / JavaScript | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` |

Também há suporte a jobs BullMQ em TypeScript/JavaScript. No RabbitMQ, um `publish` na exchange chega às filas ligadas pelos bindings. NATS considera os curingas `*` e `>`. Clusters de broker não são modelados: o mesmo broker/nome em clusters diferentes pode ligar como se pertencesse a um só.

Se o nome de um lado não pode ser avaliado, aparece como `channel::<broker>` não resolvido:
ele é mostrado, mas nunca ligado. Isso não vira automaticamente um item de URL dinâmica para
`meada decide`. A [referência de export](export.md) distingue contratos, bindings e publicações.

## Pacotes

Só os pacotes presentes no workspace fornecem código; dependência externa não dispara um
download. Cada ecossistema mantém sua própria identidade e regra de ligação.

### Pacotes npm

Cada `package.json` com nome gera `package::npm::<nome>`. Dependências em `dependencies`, `devDependencies`, `peerDependencies` e `optionalDependencies` se ligam aos pacotes do workspace, inclusive no próprio repositório. A faixa é comparada à versão local: `range mismatch` quando não aceita a versão, `range unchecked` quando não pode ser conferida.

Chamadas a funções importadas atravessam para o código do provedor por `exports`, `types`, `module`, `main` e re-exports. Se a entrada aponta ao build, o `outDir` do `tsconfig.json` é mapeado de volta ao `rootDir`. `search` com tipo `package` mostra nome, repositório, pasta, versão e consumidores. `context` de uma função exportada lista seus chamadores em todo o workspace.

### Pacotes Python

Cada `pyproject.toml` com nome em `[project]` ou `[tool.poetry]` gera `package::pypi::<nome>`, normalizado pelo PEP 503. Dependências em `[project] dependencies`, `[tool.poetry.dependencies]` e `requirements*.txt` se ligam aos pacotes do workspace. A faixa permanece como escrita, com `range unchecked`: faixas PyPI não são conferidas.

Um import chega ao código do provedor com nível `exact` quando o módulo tem o nome da distribuição, `inferred` quando é um módulo de topo dela e `ambiguous` quando várias distribuições o expõem. Pacotes npm e PyPI com o mesmo nome não se ligam.

### Pacotes Rust

Cada `Cargo.toml` com `[package] name` gera `package::cargo::<nome>`. Cada membro de um workspace Cargo é um provedor. Entradas em `[dependencies]`, `[dev-dependencies]` e `[build-dependencies]` são resolvidas assim:

- `path`: crate da pasta, no mesmo repositório ou em outro do workspace. O caminho é resolvido pelo texto a partir das raízes, sem ler fora delas nem seguir links simbólicos. Um caminho fora do workspace não aparece.
- `git`: repositório identificado pela URL normalizada.
- Versão: provedor do mesmo nome no workspace.

`package =` informa o nome real de uma dependência renomeada. `workspace = true` lê `[workspace.dependencies]`; `version.workspace = true` lê `[workspace.package]`.

A ligação é `exact`, com faixa como escrita e `range unchecked`, pois faixas Cargo não são conferidas. Um `use` segue o `[lib] name` ou a chave da dependência renomeada e atravessa os `pub use` até os itens públicos. Essa ligação também é `exact`, com `; import crate name` na razão. Dependências fora do workspace não aparecem.

### Módulos Go

Cada `go.mod` com `module` gera `package::go::<caminho do módulo>`. `require` e `replace` para pastas locais se ligam ao provedor no workspace. Um `replace` que sai do repositório é resolvido pelo texto, como `path` no Cargo; um `replace` por outro módulo fica de fora.

A ligação é `exact`, com a versão do `require` como faixa, `(local none)` como versão local e `range unchecked`. Um import de `<módulo>/<pasta>` chega às funções, tipos e métodos exportados da pasta. Quando ela existe, a ligação é `exact`, com `; import module path` na razão.

O Meada não lê `go.sum` nem lockfiles, incluindo `Cargo.lock`. Não consulta o proxy de módulos, o registro npm, PyPI ou crates.io.

## Drift de formato

Drift é um campo que um consumidor `fetch` lê da resposta, mas que a rota não devolve. O Meada compara a leitura com OpenAPI, schema zod ou objetos literais do handler.

A inferência dessas leituras de resposta é do fluxo TypeScript/JavaScript de `fetch`,
inclusive wrappers reconhecidos, e não uma promessa de inferir qualquer payload em todos
os packs. A comparação é de presença de campos, não de valores nem de compatibilidade de tipos.
Um campo vindo de tipo declarado e ausente em provedor também declarado é `exact`; outras
leituras ausentes são `inferred`. Uma parte do provedor vinda de resolução de pacote abaixo
de `exact` limita esse diagnóstico a `inferred`. Esse é o nível do diagnóstico do campo,
separado do nível da ligação entre repositórios.

Campos opcionais do tipo não são reportados como ausentes se nenhuma leitura os tornou
obrigatórios. Leituras sob uma parte aberta do provedor são `unchecked` (não conferidas),
nunca drift; ausência de um diagnóstico ali não prova que o campo existe. Veja os campos
`Shape` e `Drift` no [export JSON](export.md).

| Consulta | Resultado |
| --- | --- |
| `trace` | Acrescenta `drift: missing <caminho>` à ligação |
| `context` de uma rota | Mostra origem do formato, consumidores, drift, nível e leituras não conferidas em partes abertas |
| `search` com tipo `drift` | Lista o drift encontrado |
| `meada status` | Conta leituras ausentes no repositório consumidor; o provedor não recebe essa contagem |

## Comunidades e processos

Uma comunidade reúne símbolos que se chamam mais entre si do que ao restante. A indexação
calcula esses grupos com Leiden, sem aleatoriedade e com aritmética inteira; `meada export`
os lista. O nome vem da pasta da maioria dos membros (`src`, `src#2`); dado o mesmo workspace,
membros e nomes são determinísticos.

Cada ligação tem nível `exact`, `declared`, `inferred`, `decided` ou `ambiguous`. Uma URL não resolvida só vira ligação após uma decisão, com nível `decided`. Uma rota cujo local de serviço é desconhecido fica fora do mapa.

Um processo é a árvore de chamadas iniciada por um ponto de entrada, atravessando repositórios. São pontos de entrada as rotas, os handlers de canais e as funções ou métodos exportados sem chamadores no próprio repositório. Também entram:

- Python: `if __name__ == "__main__":` (símbolo `__main__`) e funções de `[project.scripts]` no formato `módulo:função`. Exportados são nomes públicos, sem `_` inicial, ou os listados em `__all__` literal.
- Rust: `fn main` de cada binário em `src/main.rs`, `src/bin/` ou `[[bin]] path`.
- Go: `func main` em um `package main`.

`trace` com `process::<identidade do ponto de entrada>` lê a árvore. `meada export` inclui cada processo. `search` com tipo `community` encontra comunidades pelo nome; com tipo `process`, encontra processos pelo rótulo do ponto de entrada. `context` mostra a comunidade do símbolo (ou `community: none`) e seus processos, percorrendo chamadas e ligações no sentido dos chamadores.

Um processo só é percorrido rio abaixo (`down`); `up` responde com uma nota. Um ramo para em
ciclo, nó já expandido, chamada/ligação ambígua ou no limite de **256 níveis**, marcando o
motivo; em ambiguidade, lista candidatos sem escolher um. Ligações `inferred` e `decided` continuam sendo
percorridas com seus níveis. O orçamento de resposta também pode cortar o texto. A lista de
processos em `context` usa alcance reverso, por isso pode incluir um processo cujo passo não
aparece na árvore cortada.

## Limites e próximos passos

Linguagens e clientes HTTP ou de mensagens fora das listas acima ainda não têm suporte.
Os packs de [Java (#26)](https://github.com/ClaudioMascaro/meada/issues/26),
[Kotlin (#27)](https://github.com/ClaudioMascaro/meada/issues/27),
[C# (#28)](https://github.com/ClaudioMascaro/meada/issues/28) e
[Swift (#29)](https://github.com/ClaudioMascaro/meada/issues/29) são **planejados para Release 2**,
como adições em versões menores **1.x**; não estão entregues nesta árvore. O
[ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md)
define esse recorte. Consulte o [roadmap](roadmap.md), as
[decisões](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) e o
[CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).
