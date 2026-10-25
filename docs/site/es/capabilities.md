# Capacidades y límites

Meada 0.1 indexa TypeScript, JavaScript, Python, Rust, Go y Java. Encuentra llamadas en el código y conecta repositorios mediante HTTP, paquetes y canales de mensajes. Cada vínculo indica su nivel de certeza.

El modo servidor incluye [consola](console.md), [snapshots](snapshots.md), [retención](generations.md) y [workspace gestionado](workspaces.md) mediante módulos opcionales. La consola tiene editor, diff, playground y grafo. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) y [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) ofrecen ejemplos de despliegue.

## Lenguajes y resolución

| Lenguaje | Archivos leídos |
| --- | --- |
| TypeScript y JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py` |
| Rust | `.rs` alcanzados por el árbol de módulos de cada crate |
| Go | `.go` incluidos por las reglas de build del paquete |
| Java | `.java`, resueltos por sus declaraciones de `package` |

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

## HTTP y documentos de contrato

Meada conecta consumidores y proveedores entre lenguajes, incluso detrás de gateways:

| Lenguaje | Consumidores HTTP | Proveedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch` | Next.js app router, Express |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | n/a | Spring MVC, JAX-RS |

En Java, un método de una clase `@RestController` o `@Controller` con `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` o `@RequestMapping(method = RequestMethod.X)` pasa a ser proveedor, bajo el prefijo del `@RequestMapping` de la clase. La ruta pliega constantes `static final` (también de otras clases) y `+`; cada ruta de un array es un proveedor. Un `@RequestMapping` sin `method` atiende todos los métodos (`http::ANY::<ruta>`) y se vincula a consumidores de cualquier método, con "any method" en la razón. `server.servlet.context-path` no se aplica. Las anotaciones y `RequestMethod` valen por un import de Spring, simple, estático o `import ...bind.annotation.*`; una clase del repositorio con el mismo nombre, o un import que también podría traer el nombre, las oculta. En `method` o en un array de rutas, un elemento ilegible se ignora y los demás valen.

En Java, una clase con `@Path` (`javax.ws.rs` o `jakarta.ws.rs`) es un recurso JAX-RS: un método con `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` u `@OPTIONS` pasa a ser proveedor en la ruta de la clase más el `@Path` del método, con constantes plegadas y `{id}` o `{id:regex}` como parámetro. Un método solo con `@Path` (localizador de subrecurso) no es ruta, y `@ApplicationPath` no se aplica. Los nombres valen por import, como en Spring.

En Rust, `base_url_env` puede vincular una URL base leída con `std::env::var`, `env!` o `dotenvy::var`. En Go, cubre `os.Getenv` y `os.LookupEnv`, incluidos valores por defecto con `if v == ""`, `len(v) == 0`, `!ok` o `!ok || v == ""` cuando `ok` viene del mismo `os.LookupEnv`. También sigue `strings.TrimSuffix`, `TrimRight` y `TrimSpace`.

Los recursos declarados en `meada.toml` también generan contratos. Con la tabla `[serve]`, el repositorio lee OpenAPI 3.0/3.1 y AsyncAPI 2.x/3.0, encontrados por nombre o listados en `[serve] documents`. Rutas y canales se convierten en contratos con campos de respuesta o payload. `meada status` muestra qué leyó y qué ignoró.

## Canales de mensajes

Cada productor se vincula con los handlers de sus consumidores, incluso dentro del mismo repositorio.

| Canal | TypeScript / JavaScript | Python | Rust | Go |
| --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` |

También se soportan jobs BullMQ. En RabbitMQ, un `publish` a una exchange alcanza las colas mediante sus bindings. NATS considera los comodines `*` y `>`. No se modelan clusters de broker: tópicos con el mismo nombre en clusters distintos pueden vincularse como si pertenecieran a uno solo.

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
