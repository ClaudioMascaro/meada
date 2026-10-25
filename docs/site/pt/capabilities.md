# Capacidades e limites

O Meada 0.1 indexa TypeScript, JavaScript, Python, Rust, Go, Java e Kotlin. Ele encontra chamadas no código e liga repositórios por HTTP, pacotes e canais de mensagens. Os resultados indicam o nível de certeza de cada ligação.

O modo servidor inclui [console](console.md), [snapshots](snapshots.md), [retenção](generations.md) e [workspace gerenciado](workspaces.md), em módulos opcionais. A console tem editor, diff, playground e grafo. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) e [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) trazem exemplos de implantação.

## Linguagens e resolução

| Linguagem | Arquivos lidos |
| --- | --- |
| TypeScript e JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py` |
| Rust | `.rs` alcançados pela árvore de módulos de cada crate |
| Go | `.go` incluídos pelas regras de build do pacote |
| Java | `.java`, resolvidos pelas declarações de `package` |
| Kotlin | `.kt` e `.kts` (fora `build.gradle.kts` e `settings.gradle.kts`), resolvidos pelas declarações de `package` |

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

### Kotlin

O Meada lê classes (inclusive `data`, `sealed`, `enum` e `annotation`), interfaces, objetos, companions, funções de topo, funções de extensão, propriedades com getter e métodos, inclusive aninhados. Os nomes incluem o pacote e, nas funções, os tipos escritos dos parâmetros: `example.helper(Int)`. Um companion sem nome é `Classe.Companion`, e uma função de extensão leva o tipo do receptor: `example.Point.norm()`. As chamadas de uma lambda pertencem ao símbolo que a contém.

Imports simples, com `as` e com `*` resolvem pelas declarações de pacote, e o próprio pacote é visível sem import. Chamadas sem receptor, com `this`, num objeto (`Registry.find()`) ou num companion (`Point.origin()`) são `exact` quando resta uma função pela quantidade de argumentos, contando parâmetros com valor padrão e `vararg`; sobrecargas com a mesma quantidade ficam `ambiguous`. Chamar o nome de uma classe a constrói. Dentro de uma lambda, uma chamada sem receptor ou com `this` pode ser de um receptor implícito, e resolve só por nome. Kotlin e Java do mesmo repositório se chamam pelo nome totalmente qualificado. Nomes de fora do repositório continuam externos.

Declarações que não são `private` nem `internal` compõem a superfície exportada; uma `internal` é visível em todo o repositório, e uma `private` de topo só no próprio arquivo. A indexação não precisa de compilador Kotlin, JDK ou Gradle.

## HTTP e documentos de contrato

O Meada liga consumidores e provedores, inclusive entre linguagens e atrás de gateways:

| Linguagem | Consumidores HTTP | Provedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch` | Next.js app router, Express |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit | Spring MVC, JAX-RS |
| Kotlin | Ktor client | Spring MVC, Ktor |

Em Java, um método de uma classe `@RestController` ou `@Controller` com `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` ou `@RequestMapping(method = RequestMethod.X)` vira provedor, sob o prefixo do `@RequestMapping` da classe. O caminho dobra constantes `static final` (também de outras classes) e `+`; cada caminho de um array é um provedor. Um `@RequestMapping` sem `method` atende todos os métodos (`http::ANY::<caminho>`) e liga a consumidores de qualquer método, com "any method" na razão. `server.servlet.context-path` não é aplicado. Anotações e `RequestMethod` valem por import do Spring, simples, estático ou `import ...bind.annotation.*`; uma classe do repositório com o mesmo nome, ou um import que também poderia trazer o nome, as esconde. Em `method` ou num array de caminhos, um elemento ilegível é ignorado e os demais valem.

Em Java, uma classe com `@Path` (`javax.ws.rs` ou `jakarta.ws.rs`) é um recurso JAX-RS: um método com `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` ou `@OPTIONS` vira provedor no caminho da classe somado ao `@Path` do método, com constantes dobradas e `{id}` ou `{id:regex}` como parâmetro. Um método só com `@Path` (localizador de sub-recurso) não é rota, e `@ApplicationPath` não é aplicado. Os nomes valem por import, como no Spring. Uma classe de controller ou recurso cuja família tem base externa (`extends com.corp.Base`) não gera provedor (#354), e uma interface com `@Path` é cliente, não servidor. Anotação totalmente qualificada sem import e anotação herdada de interface não são lidas.

Em Kotlin, os controladores Spring seguem as regras de Java. Uma rota Ktor dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, com caminho ou sem, e `route(caminho, HttpMethod.X)`) vira provedor sob o prefixo de cada bloco `route(prefixo) { }` em volta dela, também quando ela está numa função de extensão de `Route` ou `Routing` chamada dentro do bloco, em outro arquivo: chamada sob dois prefixos, ela serve nos dois. O lambda da rota é o handler. Os nomes do Ktor só valem com o import do Ktor; o roteamento tipado (classes de recurso) não é lido.

Em Kotlin, uma chamada do cliente Ktor (`get`, `post`, `put`, `patch`, `delete`, `head`, `options` e `request(url) { method = HttpMethod.X }`, sem método vale `GET`) num `HttpClient` é consumidor. Um `defaultRequest { url(base) }` na configuração do cliente é a base das URLs relativas. A URL dobra literais, templates (`"$base/users/${id}"`), `+`, `const val` e propriedades `val` iniciadas com uma expressão que dobra, também de outros arquivos; uma parte que não dobra vira parâmetro. Uma função que embrulha o cliente vira consumidor em cada chamada, com os argumentos substituídos, em qualquer número de níveis; uma chamada cuja URL não dá para avaliar é consumidor sem resolução, e `url { path(..) }` não é lido. O método que o bloco de `request` atribui (`method = m` ou `this.method = m`) vem do argumento do embrulho em cada chamada; uma atribuição que pode não rodar (um `if`, uma lambda dentro do bloco), um bloco que não é uma lambda escrita na chamada ou um método que não é um membro de `HttpMethod` deixa o método desconhecido, nunca `GET`. O cliente pode ser importado por `*`, escrito pelo tipo, ou guardado num `val` local sem tipo (`val c = HttpClient(CIO)`, `HttpClient(CIO).use { it.get(..) }`); um `var`, ou um local com o nome de uma propriedade do arquivo, não é lido. Os nomes valem só com o import do Ktor e um receptor que seja um `HttpClient`.

Os consumidores Java são os métodos de `RestTemplate` (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` e `exchange(url, HttpMethod.X, ...)`), o `.uri(...)` de um pedido WebClient (`.get()`, `.post()` e os demais, ou `.method(HttpMethod.X)`), e o `build()` de uma cadeia `HttpRequest.newBuilder()` (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` ou `.method("X", ...)`) ou `new Request.Builder()` do OkHttp (`.url(url)`, `.get()`, `.post(...)` e os demais ou `.method("X", ...)`); sem método na cadeia, vale `GET`. O cliente vale pelo tipo que o código escreve (campo, parâmetro, variável), importado por nome ou por um `import ...*` que seja o único a poder trazê-lo, ou pela chamada que o cria, e a base de um WebClient vem do `WebClient.create(base)` ou `WebClient.builder().baseUrl(base)` de onde ele sai, também por uma constante, por um campo `final` com um único valor ou pelo argumento que cada chamador passa a um wrapper; de um WebClient guardado numa variável local só o tipo é lido, e a base fica desconhecida. A URL dobra literais, `+`, constantes `static final` (também de outras classes), `URI.create(...)`, `String.format` (cada `%s` ou `%d` recebe o argumento correspondente, e vira parâmetro quando ele não se lê) e variáveis de template (`{id}`). Um método que monta a URL ou o método a partir dos seus parâmetros é um wrapper: cada chamada vira um consumidor com os argumentos substituídos, em qualquer número de níveis. `HttpMethod.X` vale por import do Spring, lido no arquivo que o escreve, inclusive quando chega por um wrapper; um tipo do repositório com o mesmo nome o esconde. Uma chamada cuja URL não se lê é um consumidor não resolvido.

Em Java, uma interface `@FeignClient(name = "x", url = "...")` (`org.springframework.cloud.openfeign`) com métodos anotados como no Spring MVC (`@GetMapping`, `@RequestMapping(method = ...)` e os demais) tem cada método como consumidor, no próprio método da interface: quem o chama chega ao provedor, e `trace` e `context` passam por ele. A base vem de `url` e o prefixo de `path`, ambos com placeholders `${chave}` resolvidos pela configuração do Spring, como no `@Value`. Uma `url` sem esquema recebe `http://`, como no Spring Cloud. Um `@RequestMapping` sem `method` vale `GET`, como o Feign o envia. Sem `url` o host fica sem resolver, e a razão do vínculo traz o `name` (`host unresolved (Feign client x)`); vale o provedor que atende o caminho e o método. `@RequestLine`, outros contratos do Feign e `@RequestMapping` na interface não são lidos. Em Retrofit, uma interface cujos métodos levam `@GET("x")`, `@POST` e os demais de `retrofit2.http` tem cada método como consumidor; a base vem do `Retrofit.Builder().baseUrl(base)` que cria a interface (`retrofit.create(Api.class)`), também por constante ou wrapper, um consumidor por criação, e sem criação a base fica desconhecida. Um caminho que começa com `/` troca o caminho da base (também quando ela é concatenada), e um relativo o acompanha; um caminho vazio é o `@Url` dinâmico do Retrofit e dá um consumidor sem contrato. Uma anotação do Retrofit tem o caminho só em `value`, e um caminho com `{` que não fecha no próprio segmento não é lido. `@HTTP` não é lido. As anotações valem por import, com as mesmas regras de esconder do Spring. Um método Feign ou Retrofit ambíguo (por exemplo, sem `url` e com dois repositórios que servem o caminho) ou não resolvido é um item aberto no próprio método: `search` com `kind=ambiguous` ou `kind=unresolved` o lista, e `meada decide` o resolve com o método como trecho.

Em Rust, `base_url_env` pode ligar uma URL base lida por `std::env::var`, `env!` ou `dotenvy::var`. Em Go, cobre `os.Getenv` e `os.LookupEnv`, inclusive valores padrão com `if v == ""`, `len(v) == 0`, `!ok` ou `!ok || v == ""` quando `ok` vem do mesmo `os.LookupEnv`. Também acompanha `strings.TrimSuffix`, `TrimRight` e `TrimSpace`.

Em Java, `base_url_env` liga a URL base de `System.getenv("X")` e a de um campo ou parâmetro de construtor com `@Value("${chave}")` do Spring. A chave vem do `application.properties`, do `application.yml` ou do `application.yaml` em `src/main/resources` do módulo do arquivo (o da raiz de código principal que o build Maven ou Gradle declara para ele, um `srcDirs` do Gradle incluído, ou senão o do `src/main/` em que ele está); com mais de um, vale o `.properties`, depois o `.yml`. Num `.properties`, o que vem depois de `#---` não é lido. Uma URL literal liga pelo host aos `[serve] hosts` dos pares. Um placeholder `${VAR}` que nenhuma chave preenche é a variável `VAR`, mapeada por `base_url_env`. Com padrão (`${VAR:https://web.example.com/api}`), o padrão liga pelo host quando algum par serve esse host; senão vale `VAR`, como sem padrão. O Meada nunca lê o valor de uma variável. Um campo `static`, um campo que algum arquivo atribui, ou um que um parâmetro ou uma variável local escondem, não recebe valor. Um parâmetro de construtor só recebe o valor do Spring quando nenhuma chamada no código cria a classe, `this(..)`, `super(..)` e um `new` dentro de classe anônima incluídos; com chamadas, o valor do chamador prevalece. Perfis (`application-<perfil>`), binding relaxado de variáveis e SpEL (`#{...}`) não são lidos.

Em Kotlin, `base_url_env` liga a URL base de `System.getenv("X")` e de `System.getenv()["X"]`, também por fallbacks `?:`, `!!`, parênteses e métodos de string como `trim`, `trimEnd`, `removeSuffix` ou `lowercase`, e a de uma propriedade ou propriedade de construtor com `@Value("\${key}")` do Spring (o `$` escapado em string Kotlin), lida pelo cliente Ktor. A configuração é a do Java (`application.properties` ou `application.yml` no `src/main/resources` do módulo), também em um repositório sem arquivo Java, com as mesmas regras: uma URL literal liga por host, um placeholder `${VAR}` é a variável `VAR`, um default cujo host um peer serve liga por host, e perfis, relaxed binding e SpEL não são lidos. A leitura precisa estar em uma propriedade ou na própria expressão da URL: um `val` local a uma função não é seguido. O Meada nunca lê o valor de uma variável.

Os recursos declarados em `meada.toml` também geram contratos. Com a tabela `[serve]`, o repositório lê OpenAPI 3.0/3.1 e AsyncAPI 2.x/3.0, encontrados pelo nome ou listados em `[serve] documents`. Rotas e canais viram contratos com campos de resposta ou payload. `meada status` mostra o que foi lido e ignorado.

## Canais de mensagens

Cada produtor se liga ao handler dos consumidores, inclusive no mesmo repositório.

| Canal | TypeScript / JavaScript | Python | Rust | Go | Java |
| --- | --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` | Spring Kafka |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` | Spring AMQP |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` | AWS SDK v2 |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` | jnats |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` | Jedis, Lettuce |
| JMS | | | | | Spring JMS |

Também há suporte a jobs BullMQ. No RabbitMQ, um `publish` na exchange chega às filas ligadas pelos bindings. NATS considera os curingas `*` e `>`. Clusters de broker não são modelados: tópicos com o mesmo nome em clusters diferentes podem se ligar como se pertencessem a um só.

Em Java, o cliente vale pelo tipo que o código escreve para ele (`KafkaTemplate`, `RabbitTemplate`, `JmsTemplate`, `SqsClient` ou `SqsAsyncClient`, `Connection` ou `Dispatcher` do jnats, `Jedis`, `JedisPooled`, `UnifiedJedis`, `JedisCluster` e os `RedisCommands`, `RedisAsyncCommands` e `RedisReactiveCommands` do Lettuce), e uma anotação de listener pelo import do Spring; um tipo do repositório com o mesmo nome (classe, interface ou anotação `@interface`) o esconde. Produzem `KafkaTemplate.send(tópico, ...)`, `RabbitTemplate.convertAndSend(exchange, routingKey, ...)` (com dois argumentos, ou com um `MessagePostProcessor`, publica na exchange padrão e chega à fila da chave, ou na `spring.rabbitmq.template.exchange` da configuração), `JmsTemplate.send` e `convertAndSend` com o destino, `sendMessage` com o `queueUrl` do pedido (a fila é o último segmento da URL), `publish(assunto, ...)` e `xadd(chave, ...)`. Consomem `@KafkaListener(topics = ...)`, também por `topicPartitions`, `@RabbitListener(queues = ...)`, `queuesToDeclare` e `bindings = @QueueBinding(...)`, cujas chaves viram bindings (numa `@Exchange` `direct`, o padrão, ou `fanout`, só as chaves sem curinga; numa `headers`, nenhuma), `@JmsListener(destination = ...)`, `receiveMessage`, `subscribe(assunto)` e as streams de `xreadGroup` (Jedis, de um `Map.of`) e `xreadgroup` (Lettuce, de `StreamOffset`). `BindingBuilder.bind(new Queue(..)).to(new TopicExchange(..)).with(chave)` também liga uma fila. O handler de uma anotação é o método anotado; o de um cliente que faz polling é o método com a chamada que nomeia o tópico ou a fila, mesmo que o laço de recepção fique em outro. Os nomes se avaliam como as URLs: constantes, campos `final`, os argumentos de um wrapper, um campo com `@Value("${chave}")` e os placeholders `${chave}` das anotações, lidos da configuração Spring do módulo. Um nome que não se avalia (uma variável de ambiente, SpEL, uma sobrecarga que os argumentos não distinguem) é um lado não resolvido.

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

Um `import` de uma classe de um artefato do workspace de que o repositório depende chega à classe no provedor, pela declaração `package` dos arquivos e não por um caminho: `context` num método público lista as chamadas de outros repositórios que chegam a ele, e numa classe pública as que chegam aos seus construtores e métodos, uma linha por chamada, e `trace` atravessa o salto. Vale para chamadas `Tipo.metodo()`, `new Tipo(...)`, chamadas sobre uma variável, campo ou parâmetro do tipo importado, importações `import a.b.*` e importações estáticas. O método é escolhido pelo nome e pelo número de argumentos entre as sobrecargas da classe e das superclasses e interfaces que ela tem no provedor, como o Java as junta. A ligação é `exact` quando um único provedor declara o pacote e o tipo, `ambiguous` quando vários declaram ou quando sobrecargas com o mesmo número de argumentos restam, e `inferred` quando um ancestral da classe vem de fora do repositório do provedor, porque ele pode declarar uma sobrecarga melhor; o motivo termina em `; import package`. Duas importações `import a.*` que trazem uma classe com o mesmo nome não geram ligação. As raízes de código são `src/main/java` e `src/test/java` de cada módulo Maven e os diretórios literais de `sourceSets { ... java { srcDirs = [...] } }` do Gradle (`srcDir`, `srcDirs +=` e a forma Kotlin DSL também). Classes de uma raiz de teste, ou de um conjunto que não seja `main`, nunca são importadas por um dependente; um arquivo fora de qualquer raiz ainda resolve pela declaração `package`. Diretórios calculados, ou dentro de `allprojects`, `subprojects`, `project(':x')`, `configure(...)` ou de um bloco condicional, não são lidos.

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
