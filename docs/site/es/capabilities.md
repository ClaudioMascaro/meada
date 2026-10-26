# Capacidades y límites

Meada 0.1 indexa TypeScript, JavaScript, Python, Rust, Go, Java, Kotlin y C#. Encuentra llamadas en el código y conecta repositorios mediante HTTP, paquetes y canales de mensajes. Cada vínculo indica su nivel de certeza.

El modo servidor incluye [consola](console.md), [snapshots](snapshots.md), [retención](generations.md) y [workspace gestionado](workspaces.md) mediante módulos opcionales. La consola tiene editor, diff, playground y grafo. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) y [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) ofrecen ejemplos de despliegue.

## Lenguajes y resolución

| Lenguaje | Archivos leídos |
| --- | --- |
| TypeScript y JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py` |
| Rust | `.rs` alcanzados por el árbol de módulos de cada crate |
| Go | `.go` incluidos por las reglas de build del paquete |
| Java | `.java`, resueltos por sus declaraciones de `package` |
| Kotlin | `.kt` y `.kts` (salvo `build.gradle.kts` y `settings.gradle.kts`), resueltos por sus declaraciones de `package` |
| C# | `.cs`, agrupados en proyectos por los `.csproj` de estilo SDK y resueltos por sus `namespace` |

### Rust

Cada `Cargo.toml` con `[package]` define un crate, en cualquier carpeta. Meada sigue sus `mod` sin ejecutar el toolchain Rust. Lee funciones, tipos, traits, módulos y métodos de `impl`. Un archivo que ningún `mod` alcanza queda fuera.

Llamadas como `crate::a::f()`, `Tipo::new()` y `Self::f()` siguen la ruta escrita. La resolución también sigue `use`, grupos, renombrados, globs, `pub use` y otros crates del workspace por nombre.

Para `x.m()`, Meada usa el tipo del receptor escrito en un `let`, parámetro, campo, `self` o retorno de función. Un `dyn Trait` o genérico limitado por trait llega al método del trait. Sin tipo escrito, resuelve por nombre. Los métodos ausentes de los `impl` del tipo, como los generados por `derive` o de traits externos, no producen vínculos. Los atributos de ruta tampoco se vinculan con funciones.

El análisis usa la configuración por defecto de cada crate para x86_64 Linux. Elementos con `#[cfg(..)]` falso y features fuera de `default` quedan fuera; se incluyen módulos `#[cfg(test)]`. No se indexan llamadas dentro de macros. Los contratos Rust cubren paquetes, HTTP y canales.

### Go

La ruta `module` de cada `go.mod` resuelve imports a carpetas de paquetes, incluido `replace` a una carpeta del repositorio. Un módulo anidado controla su subárbol. Archivos de una carpeta con el mismo `package` comparten ámbito, incluidos los `_test.go`. Meada no ejecuta el toolchain Go.

El análisis usa linux/amd64. Archivos `_windows.go`, con `//go:build` falso o con cgo (`import "C"`) quedan fuera. Meada lee funciones, tipos, interfaces y métodos (`Tipo.Metodo`). Resuelve llamadas dentro del paquete y mediante imports (`pkg.F()`, alias o punto); la biblioteca estándar y los módulos externos permanecen externos.

Una llamada `recv.M()` es `exact` cuando el código indica el tipo en un parámetro, campo, `var`, literal compuesto o resultado de función. Sin ello, la resolución por nombre es `inferred` o `ambiguous`. Una llamada por interfaz termina en el método de la interfaz.

### Java

Meada lee clases, interfaces, enums, records, métodos y constructores, incluidos los tipos anidados. Los nombres incluyen el paquete y, en los métodos, los tipos escritos de los parámetros: `example.Orders.place(String,int)`. Las llamadas en lambdas y clases anónimas pertenecen al símbolo que las contiene.

Los imports simples, de paquete y estáticos, incluidos los de `*`, se resuelven por las declaraciones de paquete. Las llamadas sin receptor, con `this` o estáticas son `exact` cuando la cantidad de argumentos deja un solo método; las sobrecargas con la misma cantidad quedan `ambiguous`. Los tipos externos siguen siendo externos.

Una llamada sobre un campo, parámetro o variable local de tipo escrito se resuelve en ese tipo y en los supertipos escritos en `extends` e `implements`, igual que `var x = new T(...)`, `var x = f()` y `f().g()` por el tipo de retorno declarado de `f`, `super.f()` y las llamadas sin receptor o con `this`. Cada firma viene de la primera clase que la declara, así que `f(int)` de la clase sobrescribe el de la base y un `f()` heredado sigue visible. Una llamada a través de un receptor de tipo interfaz se detiene en el método de la interfaz. Un supertipo de fuera del repositorio tiene miembros desconocidos: con uno en cualquier punto de la jerarquía, ninguno de sus métodos es `exact`, y la llamada se resuelve por nombre dentro de la jerarquía. Con el receptor desconocido, la llamada se resuelve por nombre: `inferred` cuando un solo método tiene ese nombre en el repositorio o, aparte del propio método que llama, en la jerarquía de la clase que llama, `ambiguous` con todos los candidatos cuando hay varios.

Los tipos públicos y sus miembros públicos o protegidos forman la superficie exportada.

La indexación no necesita JDK, Maven ni Gradle y no ejecuta código del repositorio.

### Kotlin

Meada lee clases (también `data`, `sealed`, `enum` y `annotation`), interfaces, objetos, companions, funciones de nivel superior, funciones de extensión, propiedades con getter y métodos, también anidados. Los nombres incluyen el paquete y, en las funciones, los tipos escritos de los parámetros: `example.helper(Int)`. Un companion sin nombre es `Clase.Companion`, y una función de extensión lleva el tipo de su receptor: `example.Point.norm()`. Las llamadas de una lambda pertenecen al símbolo que la contiene.

Los imports simples, con `as` y con `*` se resuelven por las declaraciones de paquete, y el propio paquete es visible sin import. Las llamadas sin receptor, con `this`, sobre un objeto (`Registry.find()`) o sobre un companion (`Point.origin()`) son `exact` cuando la cantidad de argumentos deja una sola función, contando los parámetros con valor por defecto y `vararg`; las sobrecargas con la misma cantidad quedan `ambiguous`. Llamar al nombre de una clase la construye. Dentro de una lambda, una llamada sin receptor o con `this` puede ser de un receptor implícito, y se resuelve solo por nombre. Kotlin y Java del mismo repositorio se llaman por el nombre totalmente calificado. Los nombres de fuera del repositorio siguen siendo externos.

Las declaraciones que no son `private` ni `internal` forman la superficie exportada; una `internal` es visible en todo el repositorio, y una `private` de nivel superior solo en su propio archivo. La indexación no necesita compilador de Kotlin, JDK ni Gradle.

### C#

Cada `.csproj` de estilo SDK (con `Sdk`) es un proyecto y contiene los `.cs` de su carpeta, dentro de un `.sln` o no; los `.sln` no cambian nada, y un `.csproj` antiguo, sin `Sdk`, no es un proyecto. Meada lee clases, records, structs e interfaces, también anidados (`Externa.Interna`) y `partial` (cada parte en su archivo, con los miembros de todas), métodos y constructores con los tipos escritos de sus parámetros (`Acme.Orders.OrderService.Place(string,int)`) y funciones locales (`Metodo.Local`). Los namespaces de bloque y de archivo dan los nombres completos. Las llamadas de una lambda pertenecen al símbolo que la contiene, y un atributo es una llamada sobre lo que decora.

`using`, `using static`, los alias (`using X = A.B`) y `global using` se resuelven por los namespaces de todos los proyectos; un `global using`, los `<Using>` del `.csproj` y los implicit usings activados (`ImplicitUsings`) valen en todos los archivos del proyecto, y solo en él. Las llamadas sin receptor, con `this`, `base`, estáticas (`Tipo.Metodo()`) y por `using static` son `exact` cuando la cantidad de argumentos deja un solo método, contando `params` y los parámetros con valor por defecto; las sobrecargas con la misma cantidad quedan `ambiguous`. Los tipos de fuera del repositorio siguen siendo externos. Las llamadas sobre un receptor de tipo escrito todavía no se resuelven por ese tipo.

Los tipos públicos y sus miembros públicos o protegidos forman la superficie exportada. La indexación no necesita .NET SDK ni MSBuild y no ejecuta código del repositorio.

## HTTP y documentos de contrato

Meada conecta consumidores y proveedores entre lenguajes, incluso detrás de gateways:

| Lenguaje | Consumidores HTTP | Proveedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch` | Next.js app router, Express |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit | Spring MVC, JAX-RS |
| Kotlin | RestTemplate, WebClient, OkHttp, Retrofit, Ktor client | Spring MVC, Ktor |
| C# | `HttpClient` | ASP.NET Core (controllers) |

En Java, un método de una clase `@RestController` o `@Controller` con `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` o `@RequestMapping(method = RequestMethod.X)` pasa a ser proveedor, bajo el prefijo del `@RequestMapping` de la clase. La ruta pliega constantes `static final` (también de otras clases) y `+`; cada ruta de un array es un proveedor. Un `@RequestMapping` sin `method` atiende todos los métodos (`http::ANY::<ruta>`) y se vincula a consumidores de cualquier método, con "any method" en la razón. `server.servlet.context-path` no se aplica. Las anotaciones y `RequestMethod` valen por un import de Spring, simple, estático o `import ...bind.annotation.*`; una clase del repositorio con el mismo nombre, o un import que también podría traer el nombre, las oculta. En `method` o en un array de rutas, un elemento ilegible se ignora y los demás valen.

En Java, una clase con `@Path` (`javax.ws.rs` o `jakarta.ws.rs`) es un recurso JAX-RS: un método con `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` u `@OPTIONS` pasa a ser proveedor en la ruta de la clase más el `@Path` del método, con constantes plegadas y `{id}` o `{id:regex}` como parámetro. Un método solo con `@Path` (localizador de subrecurso) no es ruta, y `@ApplicationPath` no se aplica. Los nombres valen por import, como en Spring. Una clase de controlador o recurso cuya familia tiene base externa (`extends com.corp.Base`) no da proveedor (#354), y una interfaz con `@Path` es cliente, no servidor. Una anotación totalmente calificada sin import y una anotación heredada de una interfaz no se leen.

En Kotlin, los controladores Spring siguen las reglas de Java. Una ruta Ktor dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, con ruta o sin ella, y `route(ruta, HttpMethod.X)`) pasa a ser proveedor bajo el prefijo de cada bloque `route(prefijo) { }` que la rodea, también cuando está en una función de extensión de `Route` o `Routing` llamada dentro del bloque, en otro archivo: llamada bajo dos prefijos, sirve en ambos. El lambda de la ruta es su handler. Los nombres de Ktor solo cuentan con el import de Ktor; el enrutamiento tipado (clases de recurso) no se lee.

En C#, un método de una clase controller (con `[ApiController]` o derivada de `ControllerBase` o `Controller`) con `[HttpGet]`, `[HttpPost]`, `[HttpPut]`, `[HttpPatch]`, `[HttpDelete]`, `[HttpHead]` o `[HttpOptions]` es un proveedor, bajo el `[Route]` de la clase. `[controller]` pasa a ser el nombre de la clase sin el sufijo `Controller`, `[action]` el nombre del método, y `{id}`, `{id:int}` y `{id?}` pasan a ser un parámetro. Una plantilla que empieza con `/` o `~/` reemplaza el prefijo de la clase. Un `[Route]` en el método, sin atributo de método, atiende todos los métodos (`http::ANY::<ruta>`) y se vincula a consumidores de cualquier método, con "any method" en la razón. Los atributos y las bases valen por `using Microsoft.AspNetCore.Mvc` o por su nombre completo (`HttpGet` y `HttpGetAttribute` son el mismo nombre); una clase del repositorio con el mismo nombre los esconde. La plantilla debe ser un literal de cadena, y un corchete literal se escribe `[[` o `]]`; cualquier otro `[token]` no genera ruta. `[action]` pierde el sufijo `Async` (como en ASP.NET Core por defecto) o vale el `[ActionName("x")]` literal. Sin `[Route]` en la clase y sin plantilla en el método, la ruta es convencional (`MapControllerRoute`), que no se lee; `[Route("")]` es la raíz. Solo cuentan los métodos públicos sin `[NonAction]`. Un `[Route]` que no es de MVC deja la clase o el método sin ruta. No generan ruta: un controller que hereda de otra clase del repositorio, una clase que es base de otra, y una clase declarada en partes (`partial`) en varios archivos.

En C#, una llamada a `GetAsync`, `PostAsync`, `PutAsync`, `PatchAsync`, `DeleteAsync`, `GetStringAsync`, `GetStreamAsync`, `GetByteArrayAsync`, `GetFromJsonAsync`, `PostAsJsonAsync`, `PutAsJsonAsync` o `SendAsync(new HttpRequestMessage(HttpMethod.X, url))` en un `HttpClient` es un consumer. El receptor solo cuenta cuando es un `HttpClient` por su tipo declarado (campo, parámetro o local tipado) o por construcción, resuelto por los usings, nunca solo por el nombre. La URL pliega literales, interpolación (`$"{base}/users/{id}"`), strings verbatim y raw, `+`, y campos `const` y `static readonly`, también de otras clases; una parte que no se puede evaluar pasa a ser parámetro. Un local ligado una vez (`var url = $"..."`) se sigue; un nombre asignado de nuevo en cualquier parte del archivo no tiene un solo valor. `BaseAddress = new Uri(base)` en el cliente, en su inicializador o por asignación (una sola), es el host de sus URLs relativas, unidas como `Uri` (`https://c/v1` y `orders` dan `/orders`), y un método que recibe la ruta (o el método, o el cliente) se lee en cada uno de sus llamadores. Una URL que no se puede evaluar es un consumer no resuelto. Los clientes de `IHttpClientFactory.CreateClient(name)` no se leen.

En Kotlin, una llamada del cliente Ktor (`get`, `post`, `put`, `patch`, `delete`, `head`, `options` y `request(url) { method = HttpMethod.X }`, `GET` si no hay método) sobre un `HttpClient` es consumidor. Un `defaultRequest { url(base) }` en la configuración del cliente es la base de las URL relativas. La URL pliega literales, plantillas (`"$base/users/${id}"`), `+`, `const val` y propiedades `val` iniciadas con una expresión que se pliega, también de otros archivos; una parte que no se pliega se vuelve parámetro. Una función que envuelve al cliente se vuelve consumidor en cada llamada, con los argumentos sustituidos, en cualquier número de niveles; una llamada cuya URL no se puede evaluar es un consumidor sin resolver, y `url { path(..) }` no se lee. El método que asigna el bloque de `request` (`method = m` o `this.method = m`) viene del argumento del envoltorio en cada llamada; una asignación que puede no ejecutarse (un `if`, una lambda dentro del bloque), un bloque que no es una lambda escrita en la llamada o un método que no es un miembro de `HttpMethod` deja el método desconocido, nunca `GET`. El cliente puede importarse con `*`, escribirse por su tipo, o guardarse en un `val` local sin tipo (`val c = HttpClient(CIO)`, `HttpClient(CIO).use { it.get(..) }`); un `var` no se lee, y un local con el nombre de un client del archivo lo oculta, con su propia base. Los nombres valen solo con el import de Ktor y un receptor que sea un `HttpClient`.

En Kotlin, `RestTemplate`, `WebClient`, OkHttp y Retrofit siguen las reglas de Java, escritas a la manera de Kotlin: `RestTemplate()` y `Request.Builder()` sin `new`, argumentos con nombre, `Api::class.java` como servicio Retrofit y una interfaz Retrofit de `fun` anotadas (las anotaciones pueden venir de `retrofit2.http.*`). El cliente puede estar en una propiedad, en una propiedad del constructor o en un `val` local (`val x = RestTemplate()`, `val web = WebClient.create(..)`), y su base es la de la llamada que lo creó. La extensión `retrofit.create<Api>()` (con `import retrofit2.create`) crea la interfaz como `create(Api::class.java)`. Un argumento con nombre en un paso anidado del builder y un builder de OkHttp en un local no se leen, y los nombres solo cuentan con el import de la biblioteca.

Los consumidores Java son los métodos de `RestTemplate` (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` y `exchange(url, HttpMethod.X, ...)`), el `.uri(...)` de una petición WebClient (`.get()`, `.post()` y los demás, o `.method(HttpMethod.X)`), y el `build()` de una cadena `HttpRequest.newBuilder()` (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` o `.method("X", ...)`) o de un `new Request.Builder()` de OkHttp (`.url(url)`, `.get()`, `.post(...)` y los demás o `.method("X", ...)`); una cadena sin método envía `GET`. El cliente vale por el tipo que el código escribe (un campo, un parámetro, una variable), importado por nombre o por un `import ...*` que sea el único que podría traerlo, o por la llamada que lo crea, y la base de un WebClient viene del `WebClient.create(base)` o `WebClient.builder().baseUrl(base)` del que sale, también a través de una constante, de un campo `final` con un único valor o del argumento que cada llamador pasa a un wrapper; de un WebClient guardado en una variable local solo se lee el tipo, y la base queda desconocida. La URL pliega literales, `+`, constantes `static final` (también de otras clases), `URI.create(...)`, `String.format` (cada `%s` o `%d` toma el argumento correspondiente, y pasa a ser un parámetro cuando no se lee) y variables de plantilla (`{id}`). Un método que arma la URL o el método a partir de sus parámetros es un wrapper: cada una de sus llamadas pasa a ser un consumidor con los argumentos sustituidos, en cualquier número de niveles. `HttpMethod.X` vale por un import de Spring, leído en el archivo que lo escribe, también cuando llega por un wrapper; un tipo del repositorio con el mismo nombre lo oculta. Una llamada cuya URL no se lee es un consumidor no resuelto.

En Java, una interfaz `@FeignClient(name = "x", url = "...")` (`org.springframework.cloud.openfeign`) con métodos anotados como en Spring MVC (`@GetMapping`, `@RequestMapping(method = ...)` y los demás) tiene cada método como consumidor, en el propio método de la interfaz: quien lo llama llega al proveedor, y `trace` y `context` pasan por él. La base viene de `url` y el prefijo de `path`, ambos con placeholders `${clave}` resueltos por la configuración de Spring, como en `@Value`. Una `url` sin esquema recibe `http://`, como en Spring Cloud. Un `@RequestMapping` sin `method` vale `GET`, como lo envía Feign. Sin `url` el host queda sin resolver, y la razón del vínculo lleva el `name` (`host unresolved (Feign client x)`); vale el proveedor que atiende la ruta y el método. `@RequestLine`, otros contratos de Feign y un `@RequestMapping` en la interfaz no se leen. En Retrofit, una interfaz cuyos métodos llevan `@GET("x")`, `@POST` y los demás de `retrofit2.http` tiene cada método como consumidor; la base viene del `Retrofit.Builder().baseUrl(base)` que crea la interfaz (`retrofit.create(Api.class)`), también por una constante o un wrapper, un consumidor por creación, y sin creación la base queda desconocida. Una ruta que empieza con `/` reemplaza la ruta de la base (también cuando está concatenada), y una relativa la sigue; una ruta vacía es el `@Url` dinámico de Retrofit y da un consumidor sin contrato. Una anotación de Retrofit tiene la ruta solo en `value`, y una ruta con una `{` que no cierra dentro de su segmento no se lee. `@HTTP` no se lee. Las anotaciones valen por import, con las mismas reglas de ocultamiento que Spring. Un método Feign o Retrofit ambiguo (por ejemplo, sin `url` y con dos repositorios que sirven la ruta) o no resuelto es un ítem abierto en el propio método: `search` con `kind=ambiguous` o `kind=unresolved` lo lista, y `meada decide` lo resuelve con el método como fragmento.

En Rust, `base_url_env` puede vincular una URL base leída con `std::env::var`, `env!` o `dotenvy::var`. En Go, cubre `os.Getenv` y `os.LookupEnv`, incluidos valores por defecto con `if v == ""`, `len(v) == 0`, `!ok` o `!ok || v == ""` cuando `ok` viene del mismo `os.LookupEnv`. También sigue `strings.TrimSuffix`, `TrimRight` y `TrimSpace`.

En Java, `base_url_env` vincula la URL base de `System.getenv("X")` y la de un campo o parámetro de constructor con `@Value("${clave}")` de Spring. La clave viene del `application.properties`, del `application.yml` o del `application.yaml` en `src/main/resources` del módulo del archivo (el de la raíz de código principal que el build Maven o Gradle declara para él, un `srcDirs` de Gradle incluido, o si no el del `src/main/` en que está); con más de uno, vale el `.properties`, después el `.yml`. En un `.properties`, lo que sigue a `#---` no se lee. Una URL literal vincula por host a los `[serve] hosts` de los pares. Un placeholder `${VAR}` que ninguna clave llena es la variable `VAR`, mapeada por `base_url_env`. Con valor por defecto (`${VAR:https://web.example.com/api}`), el valor por defecto vincula por host cuando algún par sirve ese host; si no, vale `VAR`, como sin valor por defecto. Meada nunca lee el valor de una variable. Un campo `static`, un campo que algún archivo asigna, o uno que un parámetro o una variable local ocultan, no recibe valor. Un parámetro de constructor solo recibe el valor de Spring cuando ninguna llamada en el código crea la clase, `this(..)`, `super(..)` y un `new` dentro de una clase anónima incluidos; con llamadas, prevalece el valor del llamador. Los perfiles (`application-<perfil>`), el binding relajado de variables y SpEL (`#{...}`) no se leen.

En Kotlin, `base_url_env` vincula la URL base de `System.getenv("X")` y de `System.getenv()["X"]`, también con fallbacks `?:`, `!!`, paréntesis y métodos de string como `trim`, `trimEnd`, `removeSuffix` o `lowercase`, y la de una propiedad o propiedad de constructor con `@Value("\${key}")` de Spring (el `$` escapado en un string Kotlin), leída por el cliente Ktor y por Retrofit, RestTemplate y WebClient en Kotlin. La configuración es la de Java (`application.properties` o `application.yml` en el `src/main/resources` del módulo), también en un repositorio sin archivos Java, con las mismas reglas: una URL literal vincula por host, un placeholder `${VAR}` es la variable `VAR`, un default cuyo host sirve un peer vincula por host, y los perfiles, el relaxed binding y SpEL no se leen. La lectura puede estar en una propiedad, en un `val` local de la función o en la propia expresión de la URL; un `var` no se sigue. Meada nunca lee el valor de una variable.

Los recursos declarados en `meada.toml` también generan contratos. Con la tabla `[serve]`, el repositorio lee OpenAPI 3.0/3.1 y AsyncAPI 2.x/3.0, encontrados por nombre o listados en `[serve] documents`. Rutas y canales se convierten en contratos con campos de respuesta o payload. `meada status` muestra qué leyó y qué ignoró.

## Canales de mensajes

Cada productor se vincula con los handlers de sus consumidores, incluso dentro del mismo repositorio.

| Canal | TypeScript / JavaScript | Python | Rust | Go | Java | Kotlin |
| --- | --- | --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` | Spring Kafka | Spring Kafka |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` | Spring AMQP | Spring AMQP |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` | AWS SDK v2 | AWS SDK v2 |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` | jnats | jnats |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` | Jedis, Lettuce | Jedis, Lettuce |
| JMS | | | | | Spring JMS | Spring JMS |

También se soportan jobs BullMQ. En RabbitMQ, un `publish` a una exchange alcanza las colas mediante sus bindings. NATS considera los comodines `*` y `>`. No se modelan clusters de broker: tópicos con el mismo nombre en clusters distintos pueden vincularse como si pertenecieran a uno solo.

En Java, un cliente cuenta por el tipo que el código escribe para él (`KafkaTemplate`, `RabbitTemplate`, `JmsTemplate`, `SqsClient` o `SqsAsyncClient`, `Connection` o `Dispatcher` de jnats, `Jedis`, `JedisPooled`, `UnifiedJedis`, `JedisCluster` y los `RedisCommands`, `RedisAsyncCommands` y `RedisReactiveCommands` de Lettuce), y una anotación de listener por su import de Spring; un tipo del repositorio con el mismo nombre (una clase, una interfaz o una anotación `@interface`) la oculta. Producen `KafkaTemplate.send(tópico, ...)`, `RabbitTemplate.convertAndSend(exchange, routingKey, ...)` (con dos argumentos, o con un `MessagePostProcessor`, publica en la exchange por defecto y llega a la cola que nombra su clave, o en la `spring.rabbitmq.template.exchange` de la configuración), `JmsTemplate.send` y `convertAndSend` con el destino, `sendMessage` con el `queueUrl` de la petición (la cola es el último segmento de la URL), `publish(asunto, ...)` y `xadd(clave, ...)`. Consumen `@KafkaListener(topics = ...)`, también por `topicPartitions`, `@RabbitListener(queues = ...)`, `queuesToDeclare` y `bindings = @QueueBinding(...)`, cuyas claves se vuelven bindings (en una `@Exchange` `direct`, la predeterminada, o `fanout`, solo las claves sin comodín; en una `headers`, ninguna), `@JmsListener(destination = ...)`, `receiveMessage`, `subscribe(asunto)` y los streams de `xreadGroup` (Jedis, de un `Map.of`) y `xreadgroup` (Lettuce, de `StreamOffset`). `BindingBuilder.bind(new Queue(..)).to(new TopicExchange(..)).with(clave)` también vincula una cola. El handler de una anotación es el método anotado; el de un cliente que hace polling es el método con la llamada que nombra el tópico o la cola, aunque el bucle de recepción esté en otro. Los nombres se evalúan como las URLs: constantes, campos `final`, los argumentos de un wrapper, un campo con `@Value("${clave}")` y los placeholders `${clave}` de las anotaciones, leídos de la configuración Spring del módulo. Un nombre que no se evalúa (una variable de entorno, SpEL, una sobrecarga que los argumentos no distinguen) es un lado no resuelto.

En Kotlin, los mismos clientes, anotaciones y bindings valen escritos a la manera de Kotlin, con las reglas de Java para handlers y nombres: argumentos con nombre y arrays (`@KafkaListener(topics = ["a", "b"])`, `arrayOf(..)`), anotaciones anidadas sin `@` (`QueueBinding(value = Queue("q"), exchange = Exchange(value = "x"), key = ["k"])`), `Queue("q")` sin `new` en `BindingBuilder`, `const val` de nivel superior o de un `companion object`, plantillas (`"orders-$env"` es un lado sin resolver mientras `env` no se evalúa), un cliente recibido por el constructor (`private val kafka: KafkaTemplate<String, String>`), `@Value("\${clave}")` y `mapOf(clave to id)` en los streams de Jedis y Lettuce. Una llamada sobre un receptor implícito (función de extensión del cliente, bloques `apply` y `with`) no se lee.

## Paquetes npm

Cada `package.json` con nombre genera `package::npm::<nombre>`. Entradas en `dependencies`, `devDependencies`, `peerDependencies` y `optionalDependencies` se vinculan con paquetes del workspace, incluidos los del mismo repositorio. Se compara el rango con la versión local: `range mismatch` si no la acepta, `range unchecked` si no puede comprobarse.

Las llamadas a funciones importadas llegan al código del proveedor mediante `exports`, `types`, `module`, `main` y re-exports. Si la entrada apunta al build, el `outDir` del `tsconfig.json` se mapea de vuelta al `rootDir`. `search` con tipo `package` muestra nombre, repositorio, carpeta, versión y consumidores. `context` de una función exportada lista sus llamadores en todo el workspace.

## Paquetes Python

Cada `pyproject.toml` con nombre en `[project]` o `[tool.poetry]` genera `package::pypi::<nombre>`, normalizado según PEP 503. Dependencias en `[project] dependencies`, `[tool.poetry.dependencies]` y `requirements*.txt` se vinculan con paquetes del workspace. El rango queda como está escrito, con `range unchecked`: los rangos PyPI no se comprueban.

Un import llega al proveedor como `exact` si el módulo tiene el nombre de la distribución, `inferred` si es un módulo de nivel superior de ella y `ambiguous` si varias distribuciones lo exponen. Paquetes npm y PyPI con el mismo nombre no se vinculan.

## Paquetes Rust

Cada `Cargo.toml` con `[package] name` genera `package::cargo::<nombre>`. Cada miembro de un workspace Cargo es un proveedor. Entradas en `[dependencies]`, `[dev-dependencies]` y `[build-dependencies]` se resuelven así:

- `path`: crate de esa carpeta, en el mismo repositorio o en otro del workspace. Se resuelve por texto desde las raíces, sin leer fuera de ellas ni seguir enlaces simbólicos. Una ruta fuera del workspace no aparece.
- `git`: repositorio identificado por la URL normalizada.
- Versión: proveedor del mismo nombre en el workspace.

`package =` da el nombre real de una dependencia renombrada. `workspace = true` lee `[workspace.dependencies]`; `version.workspace = true` lee `[workspace.package]`.

El vínculo es `exact`, con el rango escrito y `range unchecked`, porque no se comprueban rangos Cargo. Un `use` sigue `[lib] name` o la clave de la dependencia renombrada y los `pub use` hasta los elementos públicos. El vínculo también es `exact`, con `; import crate name` en la razón. Dependencias fuera del workspace no aparecen.

## Módulos Go

Cada `go.mod` con `module` genera `package::go::<ruta del módulo>`. `require` y `replace` a carpetas locales se vinculan con el proveedor en el workspace. Un `replace` que sale del repositorio se resuelve por texto, como `path` de Cargo; un reemplazo por otro módulo queda fuera.

El vínculo es `exact`, con la versión del `require` como rango, `(local none)` como versión local y `range unchecked`. Un import de `<módulo>/<carpeta>` llega a sus funciones, tipos y métodos exportados. Si existe la carpeta, el vínculo es `exact`, con `; import module path` en la razón.

Meada no lee `go.sum` ni lockfiles, incluido `Cargo.lock`. No consulta el proxy de módulos, el registro npm, PyPI ni crates.io.

## Paquetes Maven y Gradle

Cada `pom.xml` con coordenadas literales y cada proyecto Gradle con `group` y nombre literales genera `package::maven::<grupo>:<artefacto>`. Maven hereda el grupo literal de `<parent>` cuando hace falta; cada módulo es un artefacto. En Gradle, `rootProject.name` e `include` de `settings.gradle` o `settings.gradle.kts` nombran los proyectos de los archivos `build.gradle` o `build.gradle.kts`. Un proyecto renombrado o movido en `settings.gradle` (`project(":app").name`, `.projectDir`) y un `group` definido en `allprojects` no generan artefacto.

Las dependencias Maven y Gradle, también en notación de mapa (`group: 'g', name: 'a'`), se vinculan con artefactos del workspace con nivel `exact` y `range unchecked`. Una versión calculada (`${project.version}`, `$v`) mantiene el vínculo `exact`, con la versión tal como está escrita. Un grupo o artefacto que usa una propiedad o variable queda `ambiguous`, incluso con un solo candidato; sus partes literales filtran candidatos, y una coordenada sin ninguna parte literal no genera dependencia. Una declaración Gradle sin coordenada, como `project(":tools")`, `files(...)`, `platform(...)` o `libs.models`, no genera dependencia. Las dependencias externas quedan fuera. Los lectores se comparten con Kotlin, sin ejecutar JDK, Maven o Gradle ni consultar registros.

Un `import` de una clase de un artefacto del workspace del que depende el repositorio llega a la clase en el proveedor mediante la declaración `package` de los archivos, no por una ruta: `context` sobre un método público lista las llamadas de otros repositorios que llegan a él, y sobre una clase pública las que llegan a sus constructores y métodos, una línea por llamada, y `trace` cruza el salto. Cubre llamadas `Tipo.metodo()`, `new Tipo(...)`, llamadas sobre una variable, campo o parámetro del tipo importado, `import a.b.*` e imports estáticos. El método se elige por nombre y número de argumentos entre las sobrecargas de la clase y de las superclases e interfaces que tiene en el proveedor, como Java las combina. El vínculo es `exact` cuando un solo proveedor declara el paquete y el tipo, `ambiguous` cuando varios lo declaran o cuando quedan sobrecargas con el mismo número de argumentos, e `inferred` cuando un ancestro de la clase viene de fuera del repositorio del proveedor, porque puede declarar una sobrecarga mejor; la razón termina en `; import package`. Dos imports `import a.*` que traen una clase con el mismo nombre no generan vínculo. Las raíces de código son `src/main/java` y `src/test/java` de cada módulo Maven y los directorios literales de `sourceSets { ... java { srcDirs = [...] } }` de Gradle (`srcDir`, `srcDirs +=` y la forma Kotlin DSL también). Las clases de una raíz de pruebas, o de un conjunto distinto de `main`, nunca las importa un dependiente; un archivo fuera de toda raíz sigue resolviendo por su declaración `package`. Los directorios calculados, y los dentro de `allprojects`, `subprojects`, `project(':x')`, `configure(...)` o un bloque condicional, no se leen.

En Kotlin, un `import` de una función de nivel superior, una función de extensión o una clase de ese artefacto llega a la declaración del mismo modo: `greet("x")` elige la función del paquete importado por nombre y cantidad de argumentos, `Money(5)` llega a la clase, `Money.of(5)` a su `companion` sin nombre y `order.format()` a una función de extensión de la clase importada. Una función de extensión solo se sigue por el `import` de su nombre (o de un `import` con `*` de su paquete) y queda `exact` cuando el tipo escrito del receptor es una clase del artefacto, cuyos miembros se conocen todos, y una sola extensión de ese tipo cabe en la llamada; en un receptor de tipo desconocido o de fuera del workspace queda `inferred`, porque un miembro de ese tipo puede ocultarla. Un miembro del tipo del receptor con el mismo nombre prevalece sobre ella, y una que el propio repositorio declara no se confunde con ella. Una llamada que puede dejar fuera un argumento con valor por defecto, o que llena un `vararg`, llega a todas las sobrecargas que podría, `ambiguous` cuando hay más de una; una declaración `internal` o `private`, y una bajo una raíz de pruebas no se siguen. Las raíces de fuentes incluyen también `src/main/kotlin` y `src/test/kotlin`, y los directorios de `sourceSets` de Gradle se leen para `kotlin { srcDirs = [...] }`, `srcDir("...")`, `sourceSets["main"]` y `sourceSets.getByName("test")` como para `java`; un conjunto Kotlin es de pruebas cuando se llama `test` o termina en `Test`, y de producción cuando se llama `main` o termina en `Main`, y cualquier otro no se lee. Un `companion` con nombre (`companion object Factory`) y `setSrcDirs` no se siguen.

## Drift de formato

Drift es un campo que un consumidor `fetch` lee de una respuesta pero que la ruta no devuelve. Meada compara la lectura con OpenAPI, un schema zod o los objetos literales del handler.

| Consulta | Resultado |
| --- | --- |
| `trace` | Añade `drift: missing <ruta>` al vínculo |
| `context` de una ruta | Muestra origen del formato, consumidores, drift, nivel y lecturas no comprobadas en partes abiertas |
| `search` con tipo `drift` | Lista el drift encontrado |
| `meada status` | Cuenta lecturas ausentes en el repositorio consumidor; el proveedor no recibe esa cuenta |

## Comunidades y procesos

Una comunidad agrupa símbolos que se llaman más entre sí que al resto. `meada export` calcula los grupos con Leiden, sin aleatoriedad y con aritmética entera. El nombre viene de la carpeta de la mayoría de sus miembros (`src`, `src#2`), igual en cada máquina.

Cada vínculo tiene nivel `exact`, `declared`, `inferred`, `decided` o `ambiguous`. Una URL sin resolver solo se convierte en vínculo tras una decisión, con nivel `decided`. Una ruta cuyo lugar de servicio se desconoce queda fuera del mapa.

Un proceso es el árbol de llamadas desde un punto de entrada, entre repositorios. Son puntos de entrada las rutas, los handlers de canales y las funciones o métodos exportados sin llamadores en su propio repositorio. También se incluyen:

- Python: `if __name__ == "__main__":` (símbolo `__main__`) y funciones de `[project.scripts]` en formato `módulo:función`. Son exportados los nombres públicos, sin `_` inicial, o los listados en un `__all__` literal.
- Rust: `fn main` de cada binario en `src/main.rs`, `src/bin/` o `[[bin]] path`.
- Go: `func main` en un `package main`.

`trace` con `process::<identidad del punto de entrada>` lee el árbol. `meada export` incluye cada proceso. `search` con tipo `community` encuentra comunidades por nombre; con tipo `process`, encuentra procesos por la etiqueta de su punto de entrada. `context` muestra la comunidad del símbolo (o `community: none`) y sus procesos, recorriendo llamadas y vínculos hacia los llamadores.

## Límites y próximos pasos

Los lenguajes y clientes HTTP o de mensajes fuera de estas listas aún no tienen soporte. El [ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md) define el alcance y el orden. Consulta el [roadmap](roadmap.md), las [decisiones](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) y el [CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).
