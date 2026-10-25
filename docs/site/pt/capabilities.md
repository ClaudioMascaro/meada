# Capacidades e limites

O Meada 0.1 indexa TypeScript, JavaScript, Python, Rust, Go e Java. Ele encontra chamadas no código e liga repositórios por HTTP, pacotes e canais de mensagens. Os resultados indicam o nível de certeza de cada ligação.

O modo servidor inclui [console](console.md), [snapshots](snapshots.md), [retenção](generations.md) e [workspace gerenciado](workspaces.md), em módulos opcionais. A console tem editor, diff, playground e grafo. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) e [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) trazem exemplos de implantação.

## Linguagens e resolução

| Linguagem | Arquivos lidos |
| --- | --- |
| TypeScript e JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py` |
| Rust | `.rs` alcançados pela árvore de módulos de cada crate |
| Go | `.go` incluídos pelas regras de build do pacote |
| Java | `.java`, resolvidos pelas declarações de `package` |

### Rust

Cada `Cargo.toml` com `[package]` define um crate, em qualquer pasta. O Meada segue seus `mod` sem executar o toolchain Rust. Lê funções, tipos, traits, módulos e métodos de `impl`. Um arquivo que nenhum `mod` alcança fica de fora.

Chamadas como `crate::a::f()`, `Tipo::new()` e `Self::f()` seguem o caminho escrito no código. A resolução também acompanha `use`, grupos, renomeações, globs, `pub use` e outros crates do workspace pelo nome.

Para `x.m()`, o Meada usa o tipo do receptor quando ele aparece em um `let`, parâmetro, campo, `self` ou retorno de função. Um `dyn Trait` ou genérico limitado por trait chega ao método do trait. Sem tipo escrito, a resolução usa o nome. Métodos ausentes dos `impl` do tipo, como os gerados por `derive` ou vindos de traits externos, não geram ligação. Atributos de rota também não ligam a funções.

A análise usa a configuração padrão de cada crate para x86_64 Linux. Itens com `#[cfg(..)]` falso e features fora do `default` ficam de fora; módulos `#[cfg(test)]` entram. Chamadas dentro de macros não são indexadas. Os contratos Rust cobrem pacotes, HTTP e canais.

### Go

O caminho `module` de cada `go.mod` resolve imports para pastas de pacotes, incluindo `replace` para uma pasta do repositório. Um módulo aninhado controla sua subárvore. Arquivos na mesma pasta com o mesmo `package` compartilham escopo, inclusive `_test.go`. O Meada não executa o toolchain Go.

A análise usa linux/amd64. Arquivos `_windows.go`, com `//go:build` falso ou com cgo (`import "C"`) ficam de fora. O Meada lê funções, tipos, interfaces e métodos (`Tipo.Metodo`). Resolve chamadas no pacote e por imports (`pkg.F()`, alias ou ponto); biblioteca padrão e módulos externos continuam externos.

Uma chamada `recv.M()` é `exact` quando o código informa o tipo em parâmetro, campo, `var`, literal composto ou resultado de função. Sem isso, a resolução pelo nome é `inferred` ou `ambiguous`. Uma chamada por interface para no método da interface.

### Java

O Meada lê classes, interfaces, enums, records, métodos e construtores, inclusive tipos aninhados. Os nomes incluem o pacote e, nos métodos, os tipos escritos dos parâmetros: `example.Orders.place(String,int)`. Lambdas e classes anônimas mantêm as chamadas no símbolo que as contém.

Imports simples, de pacote e estáticos, inclusive com `*`, resolvem pelas declarações de pacote. Chamadas sem receptor, com `this` ou estáticas são `exact` quando resta um método pela quantidade de argumentos; sobrecargas com a mesma quantidade ficam `ambiguous`. Tipos externos continuam externos.

Uma chamada num campo, parâmetro ou variável local de tipo escrito resolve nesse tipo e nos supertipos escritos em `extends` e `implements`, assim como `var x = new T(...)`, `var x = f()` e `f().g()` pelo tipo de retorno declarado de `f`, `super.f()` e as chamadas sem receptor ou com `this`. Cada assinatura vem da primeira classe que a declara, então `f(int)` da classe sobrepõe o da base e `f()` herdado continua visível. Uma chamada por um receptor de interface para no método da interface. Um supertipo de fora do repositório tem membros desconhecidos: com ele em qualquer ponto da hierarquia, nenhum método dela é `exact`, e a chamada resolve por nome dentro da hierarquia. Com o receptor desconhecido, a chamada resolve por nome: `inferred` quando um só método tem esse nome no repositório ou, fora o próprio método que chama, na hierarquia da classe que faz a chamada, `ambiguous` com todos os candidatos quando há vários.

Tipos públicos e seus membros públicos ou protegidos compõem a superfície exportada.

A indexação não precisa de JDK, Maven ou Gradle e não executa código do repositório.

## HTTP e documentos de contrato

O Meada liga consumidores e provedores, inclusive entre linguagens e atrás de gateways:

| Linguagem | Consumidores HTTP | Provedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch` | Next.js app router, Express |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp | Spring MVC, JAX-RS |

Em Java, um método de uma classe `@RestController` ou `@Controller` com `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` ou `@RequestMapping(method = RequestMethod.X)` vira provedor, sob o prefixo do `@RequestMapping` da classe. O caminho dobra constantes `static final` (também de outras classes) e `+`; cada caminho de um array é um provedor. Um `@RequestMapping` sem `method` atende todos os métodos (`http::ANY::<caminho>`) e liga a consumidores de qualquer método, com "any method" na razão. `server.servlet.context-path` não é aplicado. Anotações e `RequestMethod` valem por import do Spring, simples, estático ou `import ...bind.annotation.*`; uma classe do repositório com o mesmo nome, ou um import que também poderia trazer o nome, as esconde. Em `method` ou num array de caminhos, um elemento ilegível é ignorado e os demais valem.

Em Java, uma classe com `@Path` (`javax.ws.rs` ou `jakarta.ws.rs`) é um recurso JAX-RS: um método com `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` ou `@OPTIONS` vira provedor no caminho da classe somado ao `@Path` do método, com constantes dobradas e `{id}` ou `{id:regex}` como parâmetro. Um método só com `@Path` (localizador de sub-recurso) não é rota, e `@ApplicationPath` não é aplicado. Os nomes valem por import, como no Spring. Uma classe de controller ou recurso cuja família tem base externa (`extends com.corp.Base`) não gera provedor (#354), e uma interface com `@Path` é cliente, não servidor. Anotação totalmente qualificada sem import e anotação herdada de interface não são lidas.

Os consumidores Java são os métodos de `RestTemplate` (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` e `exchange(url, HttpMethod.X, ...)`), o `.uri(...)` de um pedido WebClient (`.get()`, `.post()` e os demais, ou `.method(HttpMethod.X)`), e o `build()` de uma cadeia `HttpRequest.newBuilder()` (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` ou `.method("X", ...)`) ou `new Request.Builder()` do OkHttp (`.url(url)`, `.get()`, `.post(...)` e os demais ou `.method("X", ...)`); sem método na cadeia, vale `GET`. O cliente vale pelo tipo que o código escreve (campo, parâmetro, variável), importado por nome ou por um `import ...*` que seja o único a poder trazê-lo, ou pela chamada que o cria, e a base de um WebClient vem do `WebClient.create(base)` ou `WebClient.builder().baseUrl(base)` de onde ele sai, também por uma constante, por um campo `final` com um único valor ou pelo argumento que cada chamador passa a um wrapper; de um WebClient guardado numa variável local só o tipo é lido, e a base fica desconhecida. A URL dobra literais, `+`, constantes `static final` (também de outras classes), `URI.create(...)`, `String.format` (cada `%s` ou `%d` recebe o argumento correspondente, e vira parâmetro quando ele não se lê) e variáveis de template (`{id}`). Um método que monta a URL ou o método a partir dos seus parâmetros é um wrapper: cada chamada vira um consumidor com os argumentos substituídos, em qualquer número de níveis. `HttpMethod.X` vale por import do Spring, lido no arquivo que o escreve, inclusive quando chega por um wrapper; um tipo do repositório com o mesmo nome o esconde. Uma chamada cuja URL não se lê é um consumidor não resolvido.

Em Rust, `base_url_env` pode ligar uma URL base lida por `std::env::var`, `env!` ou `dotenvy::var`. Em Go, cobre `os.Getenv` e `os.LookupEnv`, inclusive valores padrão com `if v == ""`, `len(v) == 0`, `!ok` ou `!ok || v == ""` quando `ok` vem do mesmo `os.LookupEnv`. Também acompanha `strings.TrimSuffix`, `TrimRight` e `TrimSpace`.

Em Java, `base_url_env` liga a URL base de `System.getenv("X")` e a de um campo ou parâmetro de construtor com `@Value("${chave}")` do Spring. A chave vem do `application.properties` ou do `application.yml` em `src/main/resources` do módulo do arquivo; com os dois, vale o `.properties`. Uma URL literal liga pelo host aos `[serve] hosts` dos pares. Um placeholder `${VAR}` que nenhuma chave preenche é a variável `VAR`, mapeada por `base_url_env`. Com padrão (`${VAR:https://web.example.com/api}`), o padrão liga pelo host quando algum par serve esse host; senão vale `VAR`, como sem padrão. O Meada nunca lê o valor de uma variável. Um campo que algum arquivo atribui, ou que um parâmetro ou uma variável local escondem, não recebe valor. Um parâmetro de construtor só recebe o valor do Spring quando nenhuma chamada no código cria a classe; com chamadas, o valor do chamador prevalece. Perfis (`application-<perfil>`), binding relaxado de variáveis e SpEL (`#{...}`) não são lidos.

Os recursos declarados em `meada.toml` também geram contratos. Com a tabela `[serve]`, o repositório lê OpenAPI 3.0/3.1 e AsyncAPI 2.x/3.0, encontrados pelo nome ou listados em `[serve] documents`. Rotas e canais viram contratos com campos de resposta ou payload. `meada status` mostra o que foi lido e ignorado.

## Canais de mensagens

Cada produtor se liga ao handler dos consumidores, inclusive no mesmo repositório.

| Canal | TypeScript / JavaScript | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` |

Também há suporte a jobs BullMQ. No RabbitMQ, um `publish` na exchange chega às filas ligadas pelos bindings. NATS considera os curingas `*` e `>`. Clusters de broker não são modelados: tópicos com o mesmo nome em clusters diferentes podem se ligar como se pertencessem a um só.

## Pacotes npm

Cada `package.json` com nome gera `package::npm::<nome>`. Dependências em `dependencies`, `devDependencies`, `peerDependencies` e `optionalDependencies` se ligam aos pacotes do workspace, inclusive no próprio repositório. A faixa é comparada à versão local: `range mismatch` quando não aceita a versão, `range unchecked` quando não pode ser conferida.

Chamadas a funções importadas atravessam para o código do provedor por `exports`, `types`, `module`, `main` e re-exports. Se a entrada aponta ao build, o `outDir` do `tsconfig.json` é mapeado de volta ao `rootDir`. `search` com tipo `package` mostra nome, repositório, pasta, versão e consumidores. `context` de uma função exportada lista seus chamadores em todo o workspace.

## Pacotes Python

Cada `pyproject.toml` com nome em `[project]` ou `[tool.poetry]` gera `package::pypi::<nome>`, normalizado pelo PEP 503. Dependências em `[project] dependencies`, `[tool.poetry.dependencies]` e `requirements*.txt` se ligam aos pacotes do workspace. A faixa permanece como escrita, com `range unchecked`: faixas PyPI não são conferidas.

Um import chega ao código do provedor com nível `exact` quando o módulo tem o nome da distribuição, `inferred` quando é um módulo de topo dela e `ambiguous` quando várias distribuições o expõem. Pacotes npm e PyPI com o mesmo nome não se ligam.

## Pacotes Rust

Cada `Cargo.toml` com `[package] name` gera `package::cargo::<nome>`. Cada membro de um workspace Cargo é um provedor. Entradas em `[dependencies]`, `[dev-dependencies]` e `[build-dependencies]` são resolvidas assim:

- `path`: crate da pasta, no mesmo repositório ou em outro do workspace. O caminho é resolvido pelo texto a partir das raízes, sem ler fora delas nem seguir links simbólicos. Um caminho fora do workspace não aparece.
- `git`: repositório identificado pela URL normalizada.
- Versão: provedor do mesmo nome no workspace.

`package =` informa o nome real de uma dependência renomeada. `workspace = true` lê `[workspace.dependencies]`; `version.workspace = true` lê `[workspace.package]`.

A ligação é `exact`, com faixa como escrita e `range unchecked`, pois faixas Cargo não são conferidas. Um `use` segue o `[lib] name` ou a chave da dependência renomeada e atravessa os `pub use` até os itens públicos. Essa ligação também é `exact`, com `; import crate name` na razão. Dependências fora do workspace não aparecem.

## Módulos Go

Cada `go.mod` com `module` gera `package::go::<caminho do módulo>`. `require` e `replace` para pastas locais se ligam ao provedor no workspace. Um `replace` que sai do repositório é resolvido pelo texto, como `path` no Cargo; um `replace` por outro módulo fica de fora.

A ligação é `exact`, com a versão do `require` como faixa, `(local none)` como versão local e `range unchecked`. Um import de `<módulo>/<pasta>` chega às funções, tipos e métodos exportados da pasta. Quando ela existe, a ligação é `exact`, com `; import module path` na razão.

O Meada não lê `go.sum` nem lockfiles, incluindo `Cargo.lock`. Não consulta o proxy de módulos, o registro npm, PyPI ou crates.io.

## Pacotes Maven e Gradle

Cada `pom.xml` com coordenadas literais e cada projeto Gradle com `group` e nome literais gera `package::maven::<grupo>:<artefato>`. O Maven herda o grupo literal de `<parent>` quando necessário; cada módulo é um artefato. No Gradle, `rootProject.name` e `include` de `settings.gradle` ou `settings.gradle.kts` nomeiam os projetos dos arquivos `build.gradle` ou `build.gradle.kts`. Um projeto renomeado ou movido em `settings.gradle` (`project(":app").name`, `.projectDir`) e um `group` definido em `allprojects` não geram artefato.

Dependências Maven e Gradle, inclusive na notação de mapa (`group: 'g', name: 'a'`), se ligam aos artefatos do workspace com nível `exact` e `range unchecked`. Uma versão calculada (`${project.version}`, `$v`) mantém a ligação `exact`, com a versão como foi escrita. Um grupo ou artefato que usa propriedade ou variável fica `ambiguous`, mesmo com um único candidato; as partes literais filtram os candidatos, e uma coordenada sem nenhuma parte literal não gera dependência. Uma declaração Gradle sem coordenada, como `project(":tools")`, `files(...)`, `platform(...)` ou `libs.models`, não gera dependência. Dependências externas ficam de fora. Os leitores são compartilhados com Kotlin, sem executar JDK, Maven ou Gradle e sem consultar registros.

## Drift de formato

Drift é um campo que um consumidor `fetch` lê da resposta, mas que a rota não devolve. O Meada compara a leitura com OpenAPI, schema zod ou objetos literais do handler.

| Consulta | Resultado |
| --- | --- |
| `trace` | Acrescenta `drift: missing <caminho>` à ligação |
| `context` de uma rota | Mostra origem do formato, consumidores, drift, nível e leituras não conferidas em partes abertas |
| `search` com tipo `drift` | Lista o drift encontrado |
| `meada status` | Conta leituras ausentes no repositório consumidor; o provedor não recebe essa contagem |

## Comunidades e processos

Uma comunidade reúne símbolos que se chamam mais entre si do que ao restante. `meada export` calcula esses grupos com Leiden, sem aleatoriedade e com aritmética inteira. O nome vem da pasta da maioria dos membros (`src`, `src#2`), igual em toda máquina.

Cada ligação tem nível `exact`, `declared`, `inferred`, `decided` ou `ambiguous`. Uma URL não resolvida só vira ligação após uma decisão, com nível `decided`. Uma rota cujo local de serviço é desconhecido fica fora do mapa.

Um processo é a árvore de chamadas iniciada por um ponto de entrada, atravessando repositórios. São pontos de entrada as rotas, os handlers de canais e as funções ou métodos exportados sem chamadores no próprio repositório. Também entram:

- Python: `if __name__ == "__main__":` (símbolo `__main__`) e funções de `[project.scripts]` no formato `módulo:função`. Exportados são nomes públicos, sem `_` inicial, ou os listados em `__all__` literal.
- Rust: `fn main` de cada binário em `src/main.rs`, `src/bin/` ou `[[bin]] path`.
- Go: `func main` em um `package main`.

`trace` com `process::<identidade do ponto de entrada>` lê a árvore. `meada export` inclui cada processo. `search` com tipo `community` encontra comunidades pelo nome; com tipo `process`, encontra processos pelo rótulo do ponto de entrada. `context` mostra a comunidade do símbolo (ou `community: none`) e seus processos, percorrendo chamadas e ligações no sentido dos chamadores.

## Limites e próximos passos

Linguagens e clientes HTTP ou de mensagens fora das listas acima ainda não têm suporte. O [ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md) define o escopo e a ordem do trabalho. Consulte o [roadmap](roadmap.md), as [decisões](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) e o [CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).
