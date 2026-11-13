# Capacidades y límites

Meada 1.4 indexa TypeScript, JavaScript, Python, Rust, Go, Java, Kotlin, C# y Swift. Encuentra llamadas en el código y conecta repositorios mediante HTTP, paquetes y canales de mensajes. Cada vínculo indica su nivel de certeza.

El modo servidor incluye [consola](console.md), [snapshots](snapshots.md), [retención](generations.md) y [workspace gestionado](workspaces.md) mediante módulos opcionales. La consola tiene editor, diff, playground y grafo, y un workspace puede delegar sus entradas a [equipos](console.md#equipos). Con `accounts`, la consola autentica [cuentas locales](console.md#cuentas-locales) sin proveedor de identidad: la cuenta predeterminada `admin` cambia su contraseña en el primer inicio de sesión, las contraseñas comunes se rechazan, diez fallos seguidos bloquean una cuenta, `meada ui reset-admin` recupera la cuenta predeterminada y el operador global crea cuentas, cambia su rol, las suspende, las reactiva y restablece su contraseña, sin dejar nunca la consola sin un operador activo, y les da grupos que `[accounts.groups]` asocia a repositorios, limitando lo que lee cada cuenta y haciendo operadores de equipo a los dueños de entradas; junto a un proveedor de identidad, `break_glass = true` mantiene solo la cuenta predeterminada como [break glass](console.md#break-glass). Una cuenta con sesión iniciada genera [tokens de agente](console.md#tokens-de-agente) para los nodos y, con `proposals`, para el servidor de propuestas, válidos de 1 a 168 horas y mostrados una sola vez, que `meada mcp --listen` y `meada workspaces mcp` con `[accounts]` admiten y responde bajo la concesión de la cuenta, sin bearer compartido. Con `[accounts.github]`, la persona [verifica su login de GitHub](console.md#login-de-github-verificado) una vez, pegando un token de acceso personal que la consola usa solo para leer el login, y sus roles en GitHub pasan a concederle repositorios en la consola y en los nodos. El [catálogo de repositorios](console.md#catalogo-de-repositorios) lista, antes de que exista un manifiesto, solo lo que la persona puede seleccionar. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) y [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) ofrecen ejemplos de despliegue. Los [workspaces con nombre](named-workspaces.md) de `workspaces` pueden mantenerse como versiones (guardar, aplicar, revertir, renombrar, borrar, delegar) por sus operadores, mediante la consola y `meada workspaces`, con cada cambio comprobado contra la base que leyó. Cada workspace puede declarar sus propios peers, referencias a variables de entorno, recursos y hosts de serve para un repositorio compartido, y la consola muestra la [configuración efectiva](named-workspaces.md#configuracion-efectiva) (lo que el `meada.toml` del propio repositorio sustituye, lo que aún no rige) antes de guardar el texto. Los operadores arman ese manifiesto con [controles guiados](named-workspaces.md#autoria-guiada) (repositorios del catálogo, peers, ajustes de serve, recursos, nombres de variables de entorno) que siguen siendo un solo documento con el texto, mostrando el diff y congelando el contenido revisado hasta que confirmen. Con `proposals`, un [servidor de propuestas](named-workspaces.md#autoria-asistida-por-mcp) (`meada workspaces mcp`) permite que un agente de CLI, con su propio MCP de lectura de GitHub, liste lo que el operador puede seleccionar y valide un manifiesto como la consola lo guardaría, sin escribir nada, para que una persona lo revise y lo aplique en la consola. Los workspaces nombrados y el servidor de propuestas también funcionan con cuentas locales, con `[accounts.github]` en lugar de `[oidc.github]`.

El [grafo de la consola](console.md#infraestructura-en-el-grafo) muestra aprovisionadores y sus
raíces, enlaces de aprovisionamiento, constructor → ejecutor de imágenes y emisor → matcher de
alertas. La leyenda explica los roles; las tablas con nombres accesibles permiten navegar por
teclado, conservan nivel y motivo de los enlaces y muestran lados sin contraparte. El playground
ofrece búsquedas `value`, `iac`, `image` y `alert`.

## Lenguajes y resolución

| Lenguaje | Archivos leídos |
| --- | --- |
| TypeScript y JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py`; cada `pyproject.toml` da raíces de código (su directorio, las raíces declaradas o `src`) a los archivos bajo él |
| Rust | `.rs` alcanzados por el árbol de módulos de cada crate |
| Go | `.go` incluidos por las reglas de build del paquete |
| Java | `.java`, resueltos por sus declaraciones de `package` |
| Kotlin | `.kt` y `.kts` (salvo `build.gradle.kts` y `settings.gradle.kts`), resueltos por sus declaraciones de `package` |
| C# | `.cs`, agrupados en proyectos por los `.csproj` de estilo SDK y resueltos por sus `namespace` |
| Swift | `.swift` (salvo `Package.swift` y `Package@swift-<versión>.swift`), agrupados en módulos por los targets de `Package.swift` |

Una anotación (Java, Kotlin), un atributo (C#, o un atributo de ruta de Rust) o un decorador (Python) escrito sobre una definición es una llamada del ámbito que la envuelve, en el nivel en que resuelve. El sitio de llamada del export nombra la definición que decora (`decorates`) y `context` la lista aparte, en `annotations:`, no entre los callees.

### TypeScript y JavaScript

Meada lee funciones, clases y métodos, bindings `const`, `let` y `var` de una función o clase, y `export default`. También lee funciones definidas por asignación en el nivel superior del archivo: `obj.y = function () {}` define `obj.y`, `X.prototype.y = function () {}` define el método `X.y`, `exports.y = ...` y `module.exports.y = ...` definen `exports.y`, exportado como `y`, y `module.exports = function () {}` define el export por defecto. Un `let` o `var` solo es una definición si el archivo nunca vuelve a asignar ese nombre. Las funciones y métodos del objeto literal de un `const` de nivel superior definen `o.m`; los objetos de opciones escritos como argumento conservan sus definiciones inline. Una llamada `obj.y()` o `y()` en el mismo archivo alcanza esas definiciones, salvo `exports.y`, que un `y()` suelto no alcanza (Node no crea un `y` local). `require` y `module.exports` no son imports entre archivos, y `new F()` de una función constructora no alcanza los métodos de `F.prototype`. Un import usa los `paths` y el `baseUrl` del `tsconfig.json` (o `jsconfig.json`) más cercano, en la carpeta del archivo o encima; ambos son relativos a la carpeta de ese config, y `node_modules` se ignora. Si ese config tiene un `extends` a otro `tsconfig.json` o `jsconfig.json` del repositorio (una ruta relativa), `paths` y `baseUrl` vienen de la cadena, y gana el config más cercano que declara cada opción; un `extends` a un paquete o fuera del repositorio no se lee, y una cadena demasiado larga o circular vale solo por el config más cercano.

### Rust

Cada `Cargo.toml` con `[package]` define un crate, en cualquier carpeta. Meada sigue sus `mod` sin ejecutar el toolchain Rust. Lee funciones, tipos, traits, módulos y métodos de `impl`. Un archivo que ningún `mod` alcanza queda fuera.

Llamadas como `crate::a::f()`, `Tipo::new()` y `Self::f()` siguen la ruta escrita. La resolución también sigue `use`, grupos, renombrados, globs, `pub use` y otros crates del workspace por nombre.

Para `x.m()`, Meada usa el tipo del receptor escrito en un `let`, parámetro, campo, `self` o retorno de función. Una llamada sobre el resultado de otra (`Router::new().route(..)`, también tras `.await`, `?` o paréntesis) se resuelve por el tipo de retorno escrito de la llamada interna: tras `?`, el tipo que envuelve un `Result<T>` u `Option<T>`, y tras `.await` en una función que devuelve `impl Future<Output = T>`, `T`. Un `dyn Trait` o genérico limitado por trait llega al método del trait. Sin tipo escrito, resuelve por nombre. Los métodos ausentes de los `impl` del tipo, como los generados por `derive` o de traits externos, no producen vínculos. Los atributos de ruta tampoco se vinculan con funciones.

El análisis usa la configuración por defecto de cada crate para x86_64 Linux. Elementos con `#[cfg(..)]` falso y features fuera de `default` quedan fuera; se incluyen los módulos compilados en un build de test, no solo los `#[cfg(test)]`. No se indexan llamadas dentro de macros, con una excepción: el cuerpo entre llaves de una macro de ítems invocada en un módulo, o entre los ítems de un `impl` o de un trait, se lee como ítems de ese módulo o ítems asociados de ese bloque (funciones, tipos, bloques `impl`, `use`, `mod` y las llamadas dentro de ellos); la invocación se escribe `m!` o `crate::m!`, y cualquier otra ruta se omite. Una macro de ítems es un `macro_rules!` del propio repositorio cuyo primer brazo recibe `$($x:item)*` y devuelve cada ítem solo con atributos externos delante, como el `cfg_rt!` de tokio; los `#[cfg(..)]` que escribe el brazo valen para cada ítem. Si el nombre tiene varias definiciones en el repositorio, todas deben coincidir en los atributos. Las demás macros, incluidas las de otros crates, siguen sin leerse. Los contratos Rust cubren paquetes, HTTP y canales.

### Go

Los clientes del AWS SDK for Go v2 cuentan cuando proceden de `New` o `NewFromConfig`, incluidos alias y fábricas seguidas mediante llamadas exactas. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM y Secrets Manager usan las formas de SDK AWS de abajo. Un tipo declarado por sí solo no demuestra el origen del cliente, salvo un cliente SQS declarado `*sqs.Client`, que conserva las formas de SQS que Meada ya leía.

La ruta `module` de cada `go.mod` resuelve imports a carpetas de paquetes, incluido `replace` a una carpeta del repositorio. Un módulo anidado controla su subárbol. Archivos de una carpeta con el mismo `package` comparten ámbito, incluidos los `_test.go`. Meada no ejecuta el toolchain Go.

El análisis usa linux/amd64. Archivos `_windows.go`, con `//go:build` falso o con cgo (`import "C"`) quedan fuera. Meada lee funciones, tipos, interfaces y métodos (`Tipo.Metodo`). Resuelve llamadas dentro del paquete y mediante imports (`pkg.F()`, alias o punto); la biblioteca estándar y los módulos externos permanecen externos.

Una llamada `recv.M()` es `exact` cuando el código indica el tipo en un parámetro, campo, `var`, literal compuesto o resultado de función. Sin ello, la resolución por nombre es `inferred` o `ambiguous`. Una llamada por interfaz termina en el método de la interfaz.

### Java

El AWS SDK for Java 2.x reconoce clientes síncronos y asíncronos creados por `create()` o `builder().build()`, con solicitudes construidas mediante builders. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM y Secrets Manager usan las formas de SDK AWS de abajo. Un tipo inyectado por sí solo no demuestra su origen; las fábricas deben resolver mediante llamadas exactas.

Meada lee clases, interfaces, enums, records, métodos y constructores, incluidos los tipos anidados. Los nombres incluyen el paquete y, en los métodos, los tipos escritos de los parámetros: `example.Orders.place(String,int)`. Las llamadas en lambdas y clases anónimas pertenecen al símbolo que las contiene.

Los imports simples, de paquete y estáticos, incluidos los de `*`, se resuelven por las declaraciones de paquete. Las llamadas sin receptor, con `this` o estáticas son `exact` cuando la cantidad de argumentos deja un solo método, contando `varargs`; las sobrecargas con la misma cantidad quedan `ambiguous`. Los tipos externos siguen siendo externos.

Una llamada sobre un campo, parámetro o variable local de tipo escrito se resuelve en ese tipo y en los supertipos escritos en `extends` e `implements`, igual que `var x = new T(...)`, `var x = f()` y `f().g()` por el tipo de retorno declarado de `f`, `super.f()` y las llamadas sin receptor o con `this`. Cada firma viene de la primera clase que la declara, así que `f(int)` de la clase sobrescribe el de la base y un `f()` heredado sigue visible. Una llamada a través de un receptor de tipo interfaz se detiene en el método de la interfaz. Un supertipo de fuera del repositorio tiene miembros desconocidos: con uno en cualquier punto de la jerarquía, ninguno de sus métodos es `exact`, y la llamada se resuelve por nombre dentro de la jerarquía. La excepción son los tipos de la plataforma cuyos miembros son fijos y conocidos (`Serializable`, `Cloneable`, `Runnable`, `Comparable`, `AutoCloseable`, `Closeable`; en Kotlin también `kotlin.Comparable`): ocultan solo los nombres que declaran, así que una clase `implements Serializable, Comparable<T>` tiene llamadas `exact` a sus propias sobrecargas, y solo `compareTo` se resuelve por nombre. Con el receptor desconocido, la llamada se resuelve por nombre: `inferred` cuando un solo método tiene ese nombre en el repositorio o, aparte del propio método que llama, en la jerarquía de la clase que llama, `ambiguous` con todos los candidatos cuando hay varios.

Los tipos públicos y sus miembros públicos o protegidos forman la superficie exportada.

Un archivo se omite por demasiado amplio cuando los nombres envolventes que repite (cobrados en cada nodo bajo ellos) pasan de 4 MiB en total: queda sin símbolos ni llamadas, los demás archivos se indexan y `meada status` lo nombra. La indexación no necesita JDK, Maven ni Gradle y no ejecuta código del repositorio.

### Kotlin

Meada lee clases (también `data`, `sealed`, `enum` y `annotation`), interfaces, objetos, companions, funciones de nivel superior, funciones de extensión, propiedades con getter y métodos, también anidados. Los nombres incluyen el paquete y, en las funciones, los tipos escritos de los parámetros: `example.helper(Int)`. Un companion sin nombre es `Clase.Companion`, y una función de extensión lleva el tipo de su receptor: `example.Point.norm()`. Las llamadas de una lambda pertenecen al símbolo que la contiene. En un método, una función de extensión o un bloque de ámbito, una llamada sin receptor toma la extensión del receptor implícito que está en el ámbito antes que una función de nivel superior del mismo nombre, como lo resuelve Kotlin (`exact`, o `ambiguous` con la función cuando la extensión puede valer para cualquier receptor); el nombre de un `object` es su instancia, así que `Dispatchers.setMain(..)` alcanza una extensión importada de `Dispatchers`, mientras que el nombre de una clase mantiene companion y estáticos.

Los imports simples, con `as` y con `*` se resuelven por las declaraciones de paquete, y el propio paquete es visible sin import; un nombre que dan dos imports con `*` queda `ambiguous`. Las llamadas sin receptor, con `this`, sobre un objeto (`Registry.find()`) o sobre un companion (`Point.origin()`) son `exact` cuando la cantidad de argumentos deja una sola función, contando los parámetros con valor por defecto y `vararg`; las sobrecargas con la misma cantidad quedan `ambiguous`. Llamar al nombre de una clase la construye: la llamada llega al constructor primario o a uno secundario, elegido por la cantidad de argumentos. Una llamada sobre el resultado de otra (`f().g()`, `Box().g()`) se resuelve por el tipo de retorno escrito de `f` (`Box?` y `Box<T>` dan `Box`), o por la clase que crea el constructor; sin tipo escrito (`fun f() = Box()`) o con un tipo de fuera del repositorio, no es `exact`. Una clase `expect` y las clases `actual` del mismo nombre son una sola clase, la `expect`: los miembros que declara se resuelven en ella, y un miembro que solo declara un `actual` se resuelve solo por nombre. Dentro de una lambda, una llamada sin receptor o con `this` puede ser de un receptor implícito, y se resuelve solo por nombre. Kotlin y Java del mismo repositorio se llaman por el nombre totalmente calificado. Los nombres de fuera del repositorio siguen siendo externos.

Las declaraciones que no son `private` ni `internal` forman la superficie exportada; una `internal` es visible en todo el repositorio, y una `private` de nivel superior solo en su propio archivo. Un archivo se omite por demasiado amplio cuando los nombres envolventes que repite (cobrados en cada nodo que guarda uno) pasan de 4 MiB en total: queda sin símbolos ni llamadas, los demás archivos se indexan y `meada status` lo nombra. La indexación no necesita compilador de Kotlin, JDK ni Gradle.

### C#

Los clientes concretos del AWS SDK para .NET cuentan mediante `new`, incluidos alias y fábricas exactas. Solicitudes con inicializadores, argumentos nombrados, listas y diccionarios nombran los lados de SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM y Secrets Manager. El tipo de una interfaz inyectada por sí solo no demuestra el origen del cliente.

Cada `.csproj` de estilo SDK (con `Sdk`) es un proyecto y contiene los `.cs` de su carpeta, dentro de un `.sln` o no; los `.sln` no cambian nada, y un `.csproj` antiguo, sin `Sdk`, no es un proyecto. Meada lee clases, records, structs e interfaces, también anidados (`Externa.Interna`) y `partial` (cada parte en su archivo, con los miembros de todas), métodos y constructores con los tipos escritos de sus parámetros (`Acme.Orders.OrderService.Place(string,int)`) y funciones locales (`Metodo.Local`). Los namespaces de bloque y de archivo dan los nombres completos. Un tipo que dos proyectos del repositorio declaran con el mismo nombre completo es, en un archivo de uno de ellos, el del propio proyecto, como lo elige el compilador; en un archivo de otro proyecto, es el del proyecto que el suyo referencia (`ProjectReference`, directa o indirectamente), y el nombre queda `ambiguous` cuando los dos o ninguno están referenciados. Un tipo `internal` (también sin modificador de acceso) existe solo para los archivos de su proyecto: en un archivo de otro proyecto el nombre se vincula al tipo que ese archivo ve, como si el `internal` no existiera, y una llamada a él no tiene vínculo; lo mismo vale para un constructor o método `internal`, `private` o `private protected` de un tipo público (`InternalsVisibleTo` no se lee). Las carpetas `obj/` y `bin/` junto a un `.csproj` son salida de compilación y quedan fuera del índice, aun sin `.gitignore`. Las llamadas de una lambda pertenecen al símbolo que la contiene, y un atributo es una llamada sobre lo que decora.

`using`, `using static`, los alias (`using X = A.B`) y `global using` se resuelven por los namespaces de todos los proyectos; un `global using`, los `<Using>` del `.csproj` y los implicit usings activados (`ImplicitUsings`) valen en todos los archivos del proyecto, y solo en él. Los tipos de cada namespace que envuelve al del archivo son visibles sin `using`, del más interno al más externo (en `Acme.Query.Internal`, los de `Acme.Query.Internal`, `Acme.Query` y `Acme`) y antes que los de un `using`, en namespaces de archivo y de bloque anidados. Las llamadas sin receptor, con `this`, `base`, estáticas (`Tipo.Metodo()`) y por `using static` (que alcanzan solo los miembros y tipos anidados que declara el tipo nombrado, no los de sus bases) son `exact` cuando la cantidad de argumentos deja un solo método, contando `params` y los parámetros con valor por defecto; las sobrecargas con la misma cantidad quedan `ambiguous`. Los tipos de fuera del repositorio siguen siendo externos. Las llamadas sobre un receptor de tipo escrito todavía no se resuelven por ese tipo. Una base de fuera del repositorio deja las llamadas a los miembros de la clase sin `exact`, salvo `IDisposable`, `IAsyncDisposable`, `IEquatable` e `IComparable`, de miembros conocidos, que ocultan solo los nombres que declaran. Los elementos (`<Using>`, `PackageReference`, `ProjectReference`) y las propiedades de un `Directory.Build.props` valen en los proyectos que tiene debajo como si estuvieran en el `.csproj`, evaluados antes que los de este (el `props` más cercano oculta los de arriba, salvo que importe a su padre con `GetPathOfFileAbove`); un `ProjectReference` ahí es relativo a cada proyecto, y uno construido sobre `$(MSBuildThisFileDirectory)` cuenta como calculado. Un `props` con más de 256 `<Using>` y referencias no se lee. `nameof(x)` vale el texto del último identificador de `x` (`nameof(Pedidos.Topico)` es `"Topico"`), así que un nombre de canal o una parte de URL escritos con él son conocidos.

Los tipos públicos y sus miembros públicos o protegidos forman la superficie exportada. Un archivo se omite por demasiado amplio cuando los nombres envolventes que repite (namespace, tipos y método, la parte del nombre envolvente entero de más de 256 bytes, cobrada en cada nodo bajo él) pasan de 4 MiB en total: queda sin símbolos ni llamadas, los demás archivos se indexan y `meada status` lo nombra. Un proyecto cuyos archivos reciben más de 256 usings distintos (`global using`, elementos `<Using>` e implicit usings juntos) tiene todos sus archivos omitidos del mismo modo, y `meada status` nombra cada uno con el límite. Los archivos generados con nombres comunes, como los snapshots de modelo de Entity Framework, se leen enteros a cualquier tamaño. La indexación no necesita .NET SDK ni MSBuild y no ejecuta código del repositorio.

### Swift

Cada target de `Package.swift` (`.target`, `.executableTarget`, `.testTarget`, `.macro`, `.plugin`) es un módulo de su paquete (dos paquetes del repositorio con un target del mismo nombre dan dos módulos) con los `.swift` de su `path:` literal, o de `Sources/<nombre>` por defecto; los archivos fuera de todo target, todos en un repositorio sin `Package.swift`, forman un solo módulo. Meada lee clases, structs, enums, actors y protocolos, también anidados (`Externa.Interna`), y funciones, métodos e inicializadores con las etiquetas de sus argumentos en el nombre (`OrderService.place(order:quantity:)`, `init(baseURL:)`); las declaraciones de un mismo ámbito en un archivo (el cuerpo de un tipo y sus extensiones) con las mismas etiquetas y otros tipos llevan también sus tipos (`audit(_:Order)`). Los miembros de una extensión son del tipo extendido, en cualquier archivo del módulo, y los de una extensión de protocolo son las implementaciones por defecto de sus requisitos. Las llamadas de una closure pertenecen al símbolo que la contiene.

Los nombres se resuelven primero en el módulo del archivo y después en los módulos que importa (`import Store`), por sus declaraciones `public` y `open`, y también por las `internal` cuando el import es `@testable`; `import Store` nombra el target del paquete del propio archivo o, si no lo tiene, el del paquete más cercano en el árbol de directorios (todos cuando varios están a la misma distancia, y entonces la llamada por él es `ambiguous`); un módulo que no es target del repositorio es externo, y una declaración `internal` solo existe en su módulo y en los archivos que lo importan con `@testable` (las `fileprivate` y `private`, en ningún otro). Las llamadas sin receptor, con `self`, estáticas (`Tipo.metodo()`) y de inicializador (`Tipo(...)`, o el propio tipo cuando no declara inicializador) son `exact` cuando queda una declaración por las etiquetas, contando parámetros con valor por defecto, variádicos y closures finales; un closure final solo llena un parámetro de tipo función (también opcional o con atributos como `@escaping`), de un parámetro de tipo genérico o de un `typealias` del repositorio para un tipo función, nunca uno de un tipo que el repositorio declara ni de otro tipo escrito (`Wheel?`, `[Int]`); un parámetro escrito como un nombre de fuera del repositorio (`animation: Animation?`, `Ext.Handler`) puede ser un alias de un tipo función, así que una llamada que solo encaja en una sobrecarga por su closure final es `inferred`, o `ambiguous` entre varias, nunca `exact` (un parámetro anterior al que el closure llena de todos modos no importa); las sobrecargas con las mismas etiquetas quedan `ambiguous`. Una función con nombre capitalizado se llama como cualquier función, y una llamada escrita sin `await` no alcanza una función declarada `async`. Una llamada en una propiedad, un parámetro o una variable local de tipo escrito (`let x: T`, `x: T?`, `any P`, o un `typealias` suyo, como `StoreOf<X>` para `Store`, seguido de alias en alias hasta un límite) o inicializada con `Tipo(...)` se resuelve en ese tipo, sus extensiones y los protocolos que adopta, `exact` cuando queda una declaración por las etiquetas; por un receptor de tipo protocolo se detiene en el requisito, sin buscar los tipos que lo adoptan. Un nombre ligado dos veces en una función, por un parámetro de closure o por un patrón, o con el nombre de un parámetro de tipo, no tiene tipo conocido ahí. Una llamada en un receptor de tipo no leído se resuelve por nombre y etiquetas (`inferred` o `ambiguous`). Una llamada en un miembro implícito (`.name(..)`, como en `wait(.seconds(1))`) se resuelve igual, entre los miembros y nunca a una función o un tipo de ese nombre; `.init(..)` queda sin resolver. Un receptor de un tipo de fuera del repositorio (`session: URLSession`, `String`, o un nombre con mayúscula que nada en el repositorio declara, como `AF`) solo alcanza los miembros de las extensiones de ese tipo en el repositorio (`extension URLSession { }`), y una llamada después de un miembro de ese nombre (`URLSession.shared.data(..)`), o sobre un valor ligado a ese miembro (`let session = URLSession.shared`), no se resuelve. Un receptor o una cabeza de ruta escrita como un `typealias` de un tipo así (`typealias Anim = Animation`, también por alias de alias) alcanza las mismas extensiones. Como en Java, un tipo con una base de fuera del repositorio (`NSObject`, `View`) no tiene llamadas `exact` a sus propias sobrecargas, salvo que la base sea un tipo de miembros conocidos (`Sendable`, `Error`, `Equatable`, `Comparable`, `Hashable`, `Encodable`, `Decodable`, `Codable`, `Identifiable`, `CustomStringConvertible`, `CaseIterable`, el tipo del valor bruto de un enum o el `RouteCollection` de Vapor), que oculta solo los nombres que declara. Un archivo se omite por demasiado amplio cuando los nombres envolventes que repite (cobrados en cada nodo que guarda uno) pasan de 4 MiB en total: queda sin símbolos ni llamadas, los demás archivos se indexan y `meada status` lo nombra. La indexación no necesita toolchain de Swift ni Xcode.

## HTTP y documentos de contrato

Las declaraciones IaC también ofrecen rutas HTTP: API Gateway HTTP y REST en Terraform/OpenTofu (HCL y JSON), eventos SAM `Api`/`HttpApi` y Serverless `http`/`httpApi`. Los métodos y rutas literales usan la misma normalización de parámetros que las rutas en código. El provider apunta al símbolo del handler local cuando se resuelve con certeza, o al repositorio completo. Los consumidores de otros repositorios usan las reglas comunes de host y peers. Las referencias, métodos o rutas desconocidos no inventan rutas; las integraciones que no sean Lambda y las rutas catch-all sin una ruta finita quedan fuera del mapa (#666).

Las URLs base de la configuración o del entorno de un workload pueden nombrar un Service
Kubernetes (`orders`, `orders.shop`, `orders.shop.svc` o
`orders.shop.svc.cluster.local`, con o sin puerto) o un servicio Compose. Meada sigue
el selector del Service y la imagen o la ruta local del workload hasta el repositorio que
atiende la ruta. Estos enlaces son `inferred`, con el Service en el motivo. Los nombres
cortos usan el namespace y la raíz de evaluación del workload conocido; los candidatos
distintos quedan `ambiguous`. El entorno procede solo de las entradas indexadas del workload
que ejecuta ese código.

Un Ingress une su host y prefijo literal con la ruta del Service, como un prefijo de gateway.
Prevalece el prefijo más específico. Las reglas con anotación de rewrite, backend desconocido
o `pathType` distinto de `Prefix` quedan sin enlace (se aceptan las formas antiguas sin
`pathType`). No se interpretan rutas específicas del controller, expresiones regulares ni
hosts con comodines. No se consulta ningún cluster. Cuando ni el Service ni el Ingress llegan a
código (un Service que no selecciona ningún workload indexado, una regla sin enlace), el host
queda en manos de `[serve] hosts`: un host declarado allí sigue enlazando en `exact`.

Meada conecta consumidores y proveedores entre lenguajes, incluso detrás de gateways. Una ruta cuyo handler está en un archivo de prueba (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`, un directorio `tests` o `src/test`) no es proveedora, salvo que `meada.toml` tenga `test_routes = true` (véase [Configuración](configuration.md)):

| Lenguaje | Consumidores HTTP | Proveedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch`, `discovery.getBaseUrl` y `proxy.endpoints` de Backstage (también la forma antigua, las claves de `proxy` que empiezan por `/` cuando ningún archivo tiene `endpoints` ni un `$include` ahí, en `proxy` o en la raíz que Meada no sigue, y el endpoint escrito como texto, `'/ep': https://...`) | Next.js app router, Express, plugins de backend de Backstage |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit | Spring MVC, JAX-RS |
| Kotlin | RestTemplate, WebClient, OkHttp, Retrofit, Ktor client | Spring MVC, Ktor |
| C# | `HttpClient`, typed clients, Refit | ASP.NET Core (controllers, minimal APIs) |
| Swift | `URLSession`, Alamofire, AsyncHTTPClient | Vapor, Hummingbird |

En TypeScript y JavaScript, un `fetch` cuya URL la arma una función del repositorio es un consumidor cuando la función solo devuelve una cadena: una arrow function con cuerpo de expresión, o un cuerpo de un único `return`. La llamada (`fetch(url("api/vets"))`), o una constante que la guarda (`const u = url("api/owners"); fetch(u)`), se pliega a la cadena con los parámetros de la función reemplazados por los argumentos de la llamada, también cuando la función viene de otro módulo. Solo se pliega una llamada que resuelve exactamente. Una función de más de una instrucción, una `async`, una que devuelve algo que no es una cadena, y una cadena con una parte que no se pliega (una base leída de `process.env`, un condicional sobre una global de build) dejan el consumidor sin resolver.

En TypeScript y JavaScript, `new URL(x)` con un argumento se lee como `x`, como lo define la especificación WHATWG URL, solo para el `URL` global sin reasignación o el export nombrado `URL` de `url` o `node:url` por `import` o `require` no ocultado. Un constructor de paquete desconocido, alias de repositorio u otro export queda opaco. `u.toString()`, `u.href` y `String(u)` pasan la URL, igual que `${u}` en una template string, esté la URL en una constante o armada en el lugar (`fetch(new URL(BASE + "/api/vets").toString())`), y un wrapper de cliente llamado con una de ellas se pliega en sus puntos de llamada. Una base leída del entorno muestra su `via`. Los cambios por `u.searchParams`, `u.search` y `u.hash` conservan la ruta; una escritura en cualquier otro miembro de una constante creada con `new URL` (`u.pathname = ..`, `u.host += ..`), en cualquier punto del archivo, deja el consumidor sin resolver, igual que una mutación dentro de una función que recibe la URL, que Meada no ve. `new URL(ref, base)` con dos argumentos es la unión de ambos, como lo define la especificación WHATWG URL (RFC 3986 sección 5.2 para estas referencias): un `ref` absoluto es él mismo, `//host/x` toma el host de `ref`, `/orders` reemplaza la ruta de la base y `orders` se une al directorio de la base (`https://h/api/` da `/api/orders`, `https://h/api` da `/orders`). Sobre una base leída del entorno o un hueco, `/orders` conserva la base como cabeza (`http::GET::/orders`, con `via env:API_URL` para `process.env.API_URL`); un `ref` relativo, uno que empieza con una lectura, y una query o un fragmento solos dejan el consumidor sin resolver, y un parámetro en cualquier lado se resuelve en los puntos de llamada, como en un wrapper de cliente. `.href`, `toString()` y `String(u)` de un `new URL` de dos argumentos no se leen.

`String(u)` respeta las declaraciones locales de `String` y pasa la URL solo cuando la llamada usa la conversión global. Las asignaciones e incrementos que pueden reemplazar esa conversión dejan la URL sin resolver. Una declaración local conserva la conversión global fuera de su ámbito. Las funciones declaradas en bloques de JavaScript sin modo estricto también ocultan la conversión en la función contenedora; los módulos, las clases y `use strict` conservan el ámbito del bloque.

En Java, un método de una clase `@RestController` o `@Controller` con `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` o `@RequestMapping(method = RequestMethod.X)` pasa a ser proveedor, bajo el prefijo del `@RequestMapping` de la clase. La ruta pliega constantes `static final` (también de otras clases) y `+`; cada ruta de un array es un proveedor. Un `@RequestMapping` sin `method` atiende todos los métodos (`http::ANY::<ruta>`) y se vincula a consumidores de cualquier método, con "any method" en la razón. `server.servlet.context-path` no se aplica. Las anotaciones y `RequestMethod` valen por un import de Spring, simple, estático o `import ...bind.annotation.*`; una clase del repositorio con el mismo nombre, o un import que también podría traer el nombre, las oculta. Las anotaciones también pueden escribirse completas, sin import (`@org.springframework.web.bind.annotation.GetMapping`), salvo que el repositorio declare una clase con ese nombre. En `method` o en un array de rutas, un elemento ilegible se ignora y los demás valen.

En Java, una clase con `@Path` (`javax.ws.rs` o `jakarta.ws.rs`) es un recurso JAX-RS: un método con `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` u `@OPTIONS` pasa a ser proveedor en la ruta de la clase más el `@Path` del método, con constantes plegadas y `{id}` o `{id:regex}` como parámetro. Un método solo con `@Path` (localizador de subrecurso) no es ruta, y `@ApplicationPath` no se aplica. Los nombres valen por import, como en Spring. Un controlador o recurso puede extender una clase de fuera del repositorio (`extends com.corp.Base`): las anotaciones de los métodos se leen por los imports, y solo un tipo anidado de esa base con el nombre de la anotación se leería mal. Una interfaz con `@Path` es cliente, no servidor. Las anotaciones escritas completas, sin import, se leen; una anotación heredada de una interfaz no se lee.

En Kotlin, los controladores Spring siguen las reglas de Java, y un controlador puede extender una clase de fuera del repositorio. Una ruta Ktor dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, con ruta o sin ella, y `route(ruta, HttpMethod.X)`) pasa a ser proveedor bajo el prefijo de cada bloque `route(prefijo) { }` que la rodea, también cuando está en una función de extensión de `Route` o `Routing` llamada dentro del bloque, en otro archivo: llamada bajo dos prefijos, sirve en ambos. El lambda de la ruta es su handler. Los nombres de Ktor solo cuentan con el import de Ktor; el enrutamiento tipado (clases de recurso) no se lee.

En Swift, una ruta Vapor (`get`, `post`, `put`, `patch`, `delete` y `on(.METHOD, ruta)`) llamada en la aplicación (`Application` o `Application.make`) o en un `grouped(prefijo)` suyo es proveedor bajo el prefijo de cada `grouped` que la rodea, también cuando el constructor de rutas pasa por variables, argumentos y retornos de funciones. Un `RouteCollection` registrado con `register(collection:)` sirve las rutas de su `boot(routes:)` bajo el constructor en que se registró, y bajo cada uno cuando se registra en dos. El closure final es el handler, y `use:` acepta una función (`self.f` es el `f` del tipo que la rodea). Un `grouped(..)` cuyos argumentos son todos llamadas sobre una cabeza con mayúscula que no produce ruta (`AuthMiddleware()`, `User.authenticator()`) es la sobrecarga de middleware y no suma ruta; una lectura de miembro (`app.sessions.middleware`) deja el grupo sin leer, y una llamada estática que devuelve un componente de ruta (`grouped(Paths.api())`) se lee como middleware, así que sus rutas se sirven bajo el prefijo del padre (un falso positivo aceptado), y una ruta que Meada no lee no genera proveedor. Los nombres solo valen con el import `Vapor`.

En Python, `base_url_env` vincula también la URL base de `os.environ`, `os.getenv` y de un campo de una subclase de `BaseSettings` (pydantic-settings) del mismo archivo: la variable es `<env_prefix><CAMPO>` en mayúsculas (el `env_prefix` literal de `model_config = SettingsConfigDict(...)` o de `class Config`), o el `alias` / `validation_alias` del campo, sin prefijo. Meada nunca lee el valor de una variable.

En TypeScript, `base_url_env` vincula la URL base de `process.env.X`, `process.env["X"]` e `import.meta.env.X`, también a través de fallbacks `||` y `??` y métodos de cadena como `trim` o `replace`, y la de `config.getString('a.b')` o `getOptionalString('a.b')` de Backstage cuya clave guarda exactamente `${VAR}` en un `app-config*.yaml` del repositorio (`app-config.yaml` primero, gana el primer archivo que tiene la clave), también cuando el valor viene de un archivo parcial por `$include` (con o sin una ruta de clave después de `#`, desde la carpeta del archivo que incluye, un parcial incluyendo otro) o es un `$env: VAR`. Un `$include` con ruta absoluta, que sale del repositorio, por enlace simbólico, con `${`, de otra extensión que `.json`, `.yaml` y `.yml`, a un archivo o ruta de clave que no existe, con otra clave al lado, en ciclo, más allá de 8 inclusiones de profundidad, de 64 inclusiones o de 1 MiB injertado desde un `app-config` no se sigue: las claves bajo él quedan sin leer y `meada status` lo nombra con su motivo. `$file` no se lee. Un `app-config` que es un único `$include` es el archivo que nombra, y uno de más de un documento YAML no se lee, porque Backstage lo rechaza. La lectura puede estar en una constante, en la propia URL o en un campo de clase `readonly`, `private` o `#x` escrito una sola vez, por su inicializador o por un `this.campo = valor` simple en el cuerpo del constructor: `this.campo` en un método es esa lectura. Un campo público mutable, uno escrito dos veces (una escritura condicional cuenta como dos) o fuera del constructor, una propiedad de parámetro, un `this.campo = parametro` y un campo de un archivo JavaScript siguen sin valor. Meada nunca lee el valor de una variable.

El `context` de una función o método muestra el nivel, el motivo y el proveedor de los vínculos `inferred` de llamadas HTTP cuando la variable de entorno de la URL base selecciona un par mediante `base_url_env`, como `base URL from API_URL` y el par elegido. Esto incluye la base leída por un campo o un parcial de Backstage; editar solo el parcial actualiza el motivo y el par en la siguiente consulta. `trace` muestra el mismo motivo.

En Swift, una ruta Hummingbird (`get`, `post`, `put`, `patch`, `delete` y `on(ruta, method: .get)`) llamada en un `Router()` o en un `group(prefijo)` suyo es proveedor bajo el prefijo de cada `group` que la rodea, a cualquier profundidad, también cuando el router pasa por argumentos y retornos de funciones. El closure de la ruta es el handler, y `use:` acepta una función. Los nombres solo valen con el import `Hummingbird`; el `addRoutes` de un controlador no se lee.

En C#, una minimal API es un proveedor: `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` y `MapMethods(patrón, métodos, handler)` sobre la aplicación (`WebApplication.Create` o el `Build()` de `WebApplication.CreateBuilder`) o sobre un grupo `MapGroup(prefijo)`, con grupos anidados a cualquier profundidad. El prefijo sigue al grupo por variables locales, argumentos de método, valores de retorno y métodos de extensión llamados sobre el grupo (`group.MapOrders()`, con el namespace de la clase estática importado), así que un método de extensión llamado en dos grupos atiende bajo los dos prefijos. `RequireAuthorization()`, `WithTags(..)` y las otras convenciones comunes mantienen el grupo. Un handler lambda es un handler inline con el nombre de su ruta; un method group (`Handlers.List`) es el método que nombra. Un grupo que Meada no lee (un parámetro de lambda, como en `UseEndpoints(e => e.MapGet(..))`, un campo, una variable reasignada) no da ruta, ni siquiera en la raíz.

En C#, un método de una clase controller (con `[ApiController]` o derivada de `ControllerBase` o `Controller`) con `[HttpGet]`, `[HttpPost]`, `[HttpPut]`, `[HttpPatch]`, `[HttpDelete]`, `[HttpHead]` o `[HttpOptions]` es un proveedor, bajo el `[Route]` de la clase. `[controller]` pasa a ser el nombre de la clase sin el sufijo `Controller`, `[action]` el nombre del método, y `{id}`, `{id:int}` y `{id?}` pasan a ser un parámetro. Una plantilla que empieza con `/` o `~/` reemplaza el prefijo de la clase. Un `[Route]` en el método, sin atributo de método, atiende todos los métodos (`http::ANY::<ruta>`) y se vincula a consumidores de cualquier método, con "any method" en la razón. Los atributos y las bases valen por `using Microsoft.AspNetCore.Mvc` o por su nombre completo (`HttpGet` y `HttpGetAttribute` son el mismo nombre); una clase del repositorio con el mismo nombre los esconde. La plantilla debe ser un literal de cadena, y un corchete literal se escribe `[[` o `]]`; cualquier otro `[token]` no genera ruta. `[action]` pierde el sufijo `Async` (como en ASP.NET Core por defecto) o vale el `[ActionName("x")]` literal. Sin `[Route]` en la clase y sin plantilla en el método, la ruta es convencional (`MapControllerRoute`), que no se lee; `[Route("")]` es la raíz. Solo cuentan los métodos públicos sin `[NonAction]`. Un `[Route]` que no es de MVC deja la clase o el método sin ruta. No generan ruta: un controller que hereda de otra clase del repositorio, una clase que es base de otra, y una clase declarada en partes (`partial`) en varios archivos.

En C#, una llamada a `GetAsync`, `PostAsync`, `PutAsync`, `PatchAsync`, `DeleteAsync`, `GetStringAsync`, `GetStreamAsync`, `GetByteArrayAsync`, `GetFromJsonAsync`, `PostAsJsonAsync`, `PutAsJsonAsync` o `SendAsync(new HttpRequestMessage(HttpMethod.X, url))` en un `HttpClient` es un consumer. El receptor solo cuenta cuando es un `HttpClient` por su tipo declarado (campo, parámetro o local tipado) o por construcción, resuelto por los usings, nunca solo por el nombre. La URL pliega literales, interpolación (`$"{base}/users/{id}"`), strings verbatim y raw, `+`, y campos `const` y `static readonly`, también de otras clases; una parte que no se puede evaluar pasa a ser parámetro. Un local ligado una vez (`var url = $"..."`) se sigue; un enlace asignado de nuevo no tiene un solo valor en su propio ámbito; el mismo nombre en otro método sigue siendo independiente. `BaseAddress = new Uri(base)` en el cliente, en su inicializador o por asignación (una sola), es el host de sus URLs relativas, unidas como `Uri` (`https://c/v1` y `orders` dan `/orders`), y un método que recibe la ruta (o el método, o el cliente) se lee en cada uno de sus llamadores. Una URL que no se puede evaluar es un consumer no resuelto. Los clientes de `IHttpClientFactory.CreateClient(name)` no se leen.

En C#, un typed client registrado con `AddHttpClient<T>(c => c.BaseAddress = new Uri(base))` (también `AddHttpClient<I, T>`) da esa base al `HttpClient` que el contenedor pasa al constructor de `T` y a toda llamada sobre él dentro de `T`, también a través de un campo `readonly` que el constructor asigna. La base se evalúa como una URL y puede venir de una lectura del entorno o de la configuración; un `BaseAddress` que el constructor fija en su parámetro prevalece. Dos registros con bases distintas, una base que no se evalúa, un registro que Meada no lee (un method group, una lambda que pasa el cliente a otra llamada, un `ConfigureHttpClient` encadenado), un `BaseAddress` que el constructor fija de forma condicional o distinta del registro, o un constructor que el propio código llama (`new T(..)`, un `T t = new(..)` tipado) dejan la base desconocida. El parámetro de un constructor primario (`class T(HttpClient http)`) no recibe inyección. Cada método de una interfaz Refit con `[Get("/x")]`, `[Post]`, `[Put]`, `[Patch]`, `[Delete]` o `[Head]` (mediante `using Refit`) es un consumidor, con la base de cada `RestService.For<T>(url)` cuyo `T` es la interfaz o una interfaz que la extiende, la ruta añadida a la ruta de la propia base; quien lo llama lo alcanza por un receptor de tipo declarado (un campo, un parámetro, una variable local tipada), así que `trace` va de quien llama, por el método de la interfaz, hasta el proveedor; `var api = RestService.For<T>(..)` no da tipo a `api`. Una interfaz que ningún `RestService.For` crea, como una registrada con `AddRefitClient`, tiene base desconocida.

En Swift, una llamada `data(from:)` o `data(for:)` en una `URLSession`, `AF.request(url, method:)` (o en un `Session`) y `execute` de un `HTTPClientRequest(url:)` con `method` son consumidores; el método es GET cuando no se escribe, y un `httpMethod` asignado a un `URLRequest` pasa a ser el método. La URL pliega `URL(string:)`, `URL(string:relativeTo:)`, `appendingPathComponent`, literales, interpolación, `+` y constantes `let` (`static let` también, de otros archivos y módulos). Una función que pasa la URL, el método o la solicitud como parámetro es un wrapper, a cualquier profundidad; una URL ilegible es un consumidor no resuelto. `dataTask(with:)` no se lee.

En Kotlin, una llamada del cliente Ktor (`get`, `post`, `put`, `patch`, `delete`, `head`, `options` y `request(url) { method = HttpMethod.X }`, `GET` si no hay método) sobre un `HttpClient` es consumidor. Un `defaultRequest { url(base) }` en la configuración del cliente es la base de las URL relativas. La URL pliega literales, plantillas (`"$base/users/${id}"`), `+`, `const val` y propiedades `val` iniciadas con una expresión que se pliega, también de otros archivos; una parte que no se pliega se vuelve parámetro. Una función que envuelve al cliente se vuelve consumidor en cada llamada, con los argumentos sustituidos, en cualquier número de niveles; una llamada cuya URL no se puede evaluar es un consumidor sin resolver, y `url { path(..) }` no se lee. El método que asigna el bloque de `request` (`method = m` o `this.method = m`) viene del argumento del envoltorio en cada llamada; una asignación que puede no ejecutarse (un `if`, una lambda dentro del bloque), un bloque que no es una lambda escrita en la llamada o un método que no es un miembro de `HttpMethod` deja el método desconocido, nunca `GET`. El cliente puede importarse con `*`, escribirse por su tipo, o guardarse en un `val` local sin tipo (`val c = HttpClient(CIO)`, `HttpClient(CIO).use { it.get(..) }`); un `var` no se lee, y un local con el nombre de un client del archivo lo oculta, con su propia base. Los nombres valen solo con el import de Ktor y un receptor que sea un `HttpClient`.

En Kotlin, `RestTemplate`, `WebClient`, OkHttp y Retrofit siguen las reglas de Java, escritas a la manera de Kotlin: `RestTemplate()` y `Request.Builder()` sin `new`, argumentos con nombre, `Api::class.java` como servicio Retrofit y una interfaz Retrofit de `fun` anotadas (las anotaciones pueden venir de `retrofit2.http.*`). El cliente puede estar en una propiedad, en una propiedad del constructor o en un `val` local (`val x = RestTemplate()`, `val web = WebClient.create(..)`), y su base es la de la llamada que lo creó. La extensión `retrofit.create<Api>()` (con `import retrofit2.create`) crea la interfaz como `create(Api::class.java)`. Un argumento con nombre en un paso anidado del builder y un builder de OkHttp en un local no se leen, y los nombres solo cuentan con el import de la biblioteca.

Los consumidores Java son los métodos de `RestTemplate` (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` y `exchange(url, HttpMethod.X, ...)`), el `.uri(...)` de una petición WebClient (`.get()`, `.post()` y los demás, o `.method(HttpMethod.X)`), y el `build()` de una cadena `HttpRequest.newBuilder()` (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` o `.method("X", ...)`) o de un `new Request.Builder()` de OkHttp (`.url(url)`, `.get()`, `.post(...)` y los demás o `.method("X", ...)`); una cadena sin método envía `GET`. El cliente vale por el tipo que el código escribe (un campo, un parámetro, una variable), importado por nombre o por un `import ...*` que sea el único que podría traerlo, o por la llamada que lo crea, y la base de un WebClient viene del `WebClient.create(base)` o `WebClient.builder().baseUrl(base)` del que sale, también a través de una constante, de un campo `final` con un único valor o del argumento que cada llamador pasa a un wrapper; de un WebClient guardado en una variable local solo se lee el tipo, y la base queda desconocida. La URL pliega literales, `+`, constantes `static final` (también de otras clases), `URI.create(...)`, `String.format` (cada `%s` o `%d` toma el argumento correspondiente, y pasa a ser un parámetro cuando no se lee) y variables de plantilla (`{id}`). Un método que arma la URL o el método a partir de sus parámetros es un wrapper: cada una de sus llamadas pasa a ser un consumidor con los argumentos sustituidos, en cualquier número de niveles. `HttpMethod.X` vale por un import de Spring, leído en el archivo que lo escribe, también cuando llega por un wrapper; un tipo del repositorio con el mismo nombre lo oculta. Una llamada cuya URL no se lee es un consumidor no resuelto.

En Java, una interfaz `@FeignClient(name = "x", url = "...")` (`org.springframework.cloud.openfeign`) con métodos anotados como en Spring MVC (`@GetMapping`, `@RequestMapping(method = ...)` y los demás) tiene cada método como consumidor, en el propio método de la interfaz: quien lo llama llega al proveedor, y `trace` y `context` pasan por él. La base viene de `url` y el prefijo de `path`, ambos con placeholders `${clave}` resueltos por la configuración de Spring, como en `@Value`. Una `url` sin esquema recibe `http://`, como en Spring Cloud. Un `@RequestMapping` sin `method` vale `GET`, como lo envía Feign. Sin `url` el host queda sin resolver, y la razón del vínculo lleva el `name` (`host unresolved (Feign client x)`); vale el proveedor que atiende la ruta y el método. `@RequestLine`, otros contratos de Feign y un `@RequestMapping` en la interfaz no se leen. En Retrofit, una interfaz cuyos métodos llevan `@GET("x")`, `@POST` y los demás de `retrofit2.http` tiene cada método como consumidor; la base viene del `Retrofit.Builder().baseUrl(base)` que crea la interfaz (`retrofit.create(Api.class)`), también por una constante o un wrapper, un consumidor por creación, y sin creación la base queda desconocida. Una ruta que empieza con `/` reemplaza la ruta de la base (también cuando está concatenada), y una relativa la sigue; una ruta vacía es el `@Url` dinámico de Retrofit y da un consumidor sin contrato. Una anotación de Retrofit tiene la ruta solo en `value`, y una ruta con una `{` que no cierra dentro de su segmento no se lee. `@HTTP` no se lee. Las anotaciones valen por import, con las mismas reglas de ocultamiento que Spring. Un método Feign o Retrofit ambiguo (por ejemplo, sin `url` y con dos repositorios que sirven la ruta) o no resuelto es un ítem abierto en el propio método: `search` con `kind=ambiguous` o `kind=unresolved` lo lista, y `meada decide` lo resuelve con el método como fragmento.

En Rust, `base_url_env` puede vincular una URL base leída con `std::env::var`, `env!` o `dotenvy::var`. En Go, cubre `os.Getenv` y `os.LookupEnv`, incluidos valores por defecto con `if v == ""`, `len(v) == 0`, `!ok` o `!ok || v == ""` cuando `ok` viene del mismo `os.LookupEnv`. También sigue `strings.TrimSuffix`, `TrimRight` y `TrimSpace`, y un campo de struct del mismo archivo con la etiqueta `env:"X"` o `envconfig:"X"` (cleanenv, caarlos0/env, envconfig). Un campo de struct que guarda la URL base (`c.base`) es la lectura que le da todo composite literal del repositorio que lo llena, directamente (`&Client{base: os.Getenv("X")}`) o por el parámetro de un constructor (`New(base string)`) cuyos llamadores en el repositorio pasan todos la misma lectura; vale en un método que lee el campo directamente o por un wrapper de petición. El campo queda desconocido cuando dos literales o dos llamadores dan lecturas distintas, cuando un literal lo llena por posición o con un tipo que el código no dice (un alias), cuando ningún literal lo llena, cuando algún archivo lo escribe por un selector (`c.base = x`), cuando el constructor no tiene llamador en el repositorio y cuando un valor es una cadena literal en lugar de una lectura.

En Java, `base_url_env` vincula la URL base de `System.getenv("X")`, también cuando se sigue una variable local que guarda la lectura (una que el archivo nunca vuelve a asignar), y la de un campo o parámetro de constructor con `@Value("${clave}")` de Spring. La clave viene del `application.properties`, del `application.yml` o del `application.yaml` en `src/main/resources` del módulo del archivo (el de la raíz de código principal que el build Maven o Gradle declara para él, un `srcDirs` de Gradle incluido, o si no el del `src/main/` en que está); con más de uno, vale el `.properties`, después el `.yml`. En un `.properties`, lo que sigue a `#---` no se lee. Una URL literal vincula por host a los `[serve] hosts` de los pares. Un placeholder `${VAR}` que ninguna clave llena es la variable `VAR`, mapeada por `base_url_env`. Con valor por defecto (`${VAR:https://web.example.com/api}`), el valor por defecto vincula por host cuando algún par sirve ese host; si no, vale `VAR`, como sin valor por defecto. Meada nunca lee el valor de una variable. Un campo `static`, un campo que algún archivo asigna, o uno que un parámetro o una variable local ocultan, no recibe valor. Un parámetro de constructor solo recibe el valor de Spring cuando ninguna llamada en el código crea la clase, `this(..)`, `super(..)` y un `new` dentro de una clase anónima incluidos; con llamadas, prevalece el valor del llamador. Los perfiles (`application-<perfil>`), el binding relajado de variables y SpEL (`#{...}`) no se leen.

En Kotlin, `base_url_env` vincula la URL base de `System.getenv("X")` y de `System.getenv()["X"]`, también con fallbacks `?:`, `!!`, paréntesis y métodos de string como `trim`, `trimEnd`, `removeSuffix` o `lowercase`, y la de una propiedad o propiedad de constructor con `@Value("\${key}")` de Spring (el `$` escapado en un string Kotlin), leída por el cliente Ktor y por Retrofit, RestTemplate y WebClient en Kotlin. La configuración es la de Java (`application.properties` o `application.yml` en el `src/main/resources` del módulo), también en un repositorio sin archivos Java, con las mismas reglas: una URL literal vincula por host, un placeholder `${VAR}` es la variable `VAR`, un default cuyo host sirve un peer vincula por host, y los perfiles, el relaxed binding y SpEL no se leen. La lectura puede estar en una propiedad, en un `val` local de la función o en la propia expresión de la URL; un `var` no se sigue. Meada nunca lee el valor de una variable.

En C#, `base_url_env` vincula la URL base de `Environment.GetEnvironmentVariable("X")` (también `System.Environment`), con fallbacks `??`, `!` y `Trim`, `TrimStart` o `TrimEnd`, y la de `configuration["Seccion:Clave"]` en un receptor declarado `IConfiguration` o en `builder.Configuration` de un `WebApplicationBuilder`, leída por las llamadas de `HttpClient` y por el `BaseAddress` de un cliente. Una clave de configuración es la variable `Seccion__Clave`, como la nombra el proveedor de entorno de .NET y como el código escribe la clave (`base_url_env` la compara tal como está escrita, aunque .NET ignora mayúsculas). La lectura puede estar en un local o un campo. Una cadena `??` cuenta cuando exactamente uno de sus operandos es una lectura. La clave se busca en el `appsettings.json` del proyecto (se aceptan comentarios, comas finales y la marca de orden de bytes, los objetos anidados se unen con `:` y las claves se comparan sin distinguir mayúsculas); una URL literal que contiene es el valor por defecto de esa variable: cuando el `[serve] hosts` de un peer nombra su host, la llamada vincula por host, y si no por `base_url_env`. `appsettings.<Entorno>.json`, `GetValue`, `GetSection`, el binding de opciones y los user secrets no se leen, y una lectura pasada por un wrapper de otro archivo es un parámetro. Meada nunca lee el valor de una variable.

En Swift, `base_url_env` vincula la URL base de `ProcessInfo.processInfo.environment["X"]` (también `Foundation.ProcessInfo`), también mediante `!`, paréntesis, fallbacks `??` y métodos de string como `trimmingCharacters`, `replacingOccurrences`, `lowercased` o `uppercased`. Un `??` cuenta cuando su fallback es un literal de string simple; una segunda lectura en el fallback o en un argumento, un fallback que es una llamada o un nombre, y una clave que no es literal dejan la base desconocida. La lectura puede estar guardada en un `let` (local, `static let` o de nivel superior), en un `if let` o `guard let` solo de ella o aparecer en la propia URL, también dentro de una interpolación. Un `var` asignado de nuevo, un `if var` o `guard var` y un optional binding de cualquier cosa que no sea una lectura no se siguen, y el `Environment.get("X")` de Vapor no es una lectura. Meada nunca lee el valor de una variable.

Los recursos declarados en `meada.toml` también generan contratos. Con la tabla `[serve]`, el repositorio lee OpenAPI 3.0/3.1 y AsyncAPI 2.x/3.0, encontrados por nombre o listados en `[serve] documents`. Rutas y canales se convierten en contratos con campos de respuesta o payload. `meada status` muestra qué leyó y qué ignoró.

## Canales de mensajes

Cada productor se vincula con los handlers de sus consumidores, incluso dentro del mismo repositorio. Swift todavía no tiene soporte de canales en la v1.

| Canal | TypeScript / JavaScript | Python | Rust | Go | Java | Kotlin | C# |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` | Spring Kafka | Spring Kafka | `Confluent.Kafka` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` | Spring AMQP | Spring AMQP | `RabbitMQ.Client` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` | AWS SDK v2 | AWS SDK v2 | `AWSSDK.SQS` |
| Amazon SNS | AWS SDK v3 `PublishCommand`, `PublishBatchCommand` | `boto3` `publish`, `publish_batch` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon EventBridge | AWS SDK v3 `PutEventsCommand` | `boto3` `put_events` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon Kinesis | AWS SDK v3 `PutRecordCommand`, `PutRecordsCommand` | `boto3` `put_record`, `put_records` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` | jnats | jnats | `NATS.Client` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` | Jedis, Lettuce | Jedis, Lettuce | `StackExchange.Redis` |
| JMS | | | | | Spring JMS | Spring JMS | |
| Azure Service Bus | Azure SDK | Azure SDK | | Azure SDK | Azure SDK | | `Azure.Messaging.ServiceBus` |

Los brokers son los ids de los contratos (`channel::<broker>::<nombre>`): `kafka`, `amqp` (RabbitMQ), `jms`, `sqs`, `nats`, `redis`, `bullmq` y `servicebus` (Azure Service Bus). También se soportan jobs BullMQ. En RabbitMQ, un `publish` a una exchange alcanza las colas mediante sus bindings. NATS considera los comodines `*` y `>`. No se modelan clusters de broker: tópicos con el mismo nombre en clusters distintos pueden vincularse como si pertenecieran a uno solo.

En Java, fuera de las formas AWS, un cliente cuenta por el tipo que el código escribe para él (`KafkaTemplate`, `RabbitTemplate`, `JmsTemplate`, `Connection` o `Dispatcher` de jnats, `Jedis`, `JedisPooled`, `UnifiedJedis`, `JedisCluster` y los `RedisCommands`, `RedisAsyncCommands` y `RedisReactiveCommands` de Lettuce), y una anotación de listener por su import de Spring; un tipo del repositorio con el mismo nombre (una clase, una interfaz o una anotación `@interface`) la oculta. Producen `KafkaTemplate.send(tópico, ...)`, `RabbitTemplate.convertAndSend(exchange, routingKey, ...)` (con dos argumentos, o con un `MessagePostProcessor`, publica en la exchange por defecto y llega a la cola que nombra su clave, o en la `spring.rabbitmq.template.exchange` de la configuración), `JmsTemplate.send` y `convertAndSend` con el destino, `sendMessage` con el `queueUrl` de la petición (la cola es el último segmento de la URL), `publish(asunto, ...)` y `xadd(clave, ...)`. Consumen `@KafkaListener(topics = ...)`, también por `topicPartitions`, `@RabbitListener(queues = ...)`, `queuesToDeclare` y `bindings = @QueueBinding(...)`, cuyas claves se vuelven bindings (en una `@Exchange` `direct`, la predeterminada, o `fanout`, solo las claves sin comodín; en una `headers`, ninguna), `@JmsListener(destination = ...)`, `receiveMessage`, `subscribe(asunto)` y los streams de `xreadGroup` (Jedis, de un `Map.of`) y `xreadgroup` (Lettuce, de `StreamOffset`). `BindingBuilder.bind(new Queue(..)).to(new TopicExchange(..)).with(clave)` también vincula una cola. El handler de una anotación es el método anotado; el de un cliente que hace polling es el método con la llamada que nombra el tópico o la cola, aunque el bucle de recepción esté en otro. Los nombres se evalúan como las URLs: constantes, campos `final`, los argumentos de un wrapper, un campo con `@Value("${clave}")` y los placeholders `${clave}` de las anotaciones, leídos de la configuración Spring del módulo. Un nombre que no se evalúa (una variable de entorno, SpEL, una sobrecarga que los argumentos no distinguen) es un lado no resuelto.

En Kotlin, los mismos clientes, anotaciones y bindings valen escritos a la manera de Kotlin, con las reglas de Java para handlers y nombres: argumentos con nombre y arrays (`@KafkaListener(topics = ["a", "b"])`, `arrayOf(..)`), anotaciones anidadas sin `@` (`QueueBinding(value = Queue("q"), exchange = Exchange(value = "x"), key = ["k"])`), `Queue("q")` sin `new` en `BindingBuilder`, `const val` de nivel superior o de un `companion object`, plantillas (`"orders-$env"` es un lado sin resolver mientras `env` no se evalúa), un cliente recibido por el constructor (`private val kafka: KafkaTemplate<String, String>`), `@Value("\${clave}")` y `mapOf(clave to id)` en los streams de Jedis y Lettuce. Una llamada sobre un receptor implícito (función de extensión del cliente, bloques `apply` y `with`) no se lee.

En C#, fuera de las formas AWS, un cliente cuenta por el tipo que el código escribe para él (un campo, una propiedad, un parámetro o un local tipado) o por la construcción que lo creó (`new ProducerBuilder<..>(..).Build()`, `factory.CreateConnection()`, `connection.CreateModel()`, `multiplexer.GetDatabase()`, `client.CreateSender(..)`), resuelto por los usings, nunca solo por el nombre: un tipo del repositorio con el mismo nombre lo oculta, y `ConnectionFactory` o `IConnection` con `RabbitMQ.Client` y `NATS.Client` importados a la vez no es ninguno de los dos. Producen `Produce` y `ProduceAsync(tópico, ...)` de Confluent.Kafka, `BasicPublish` y `BasicPublishAsync(exchange, routingKey, ...)` (la exchange vacía, `string.Empty`, `String.Empty` con `using System;` o `""`, llega a la cola que nombra su clave), `SendMessageAsync` de AWSSDK.SQS con la URL de la cola o con `new SendMessageRequest { QueueUrl = .. }` (la cola es el último segmento de la URL), `Publish(asunto, ...)` de NATS.Client, `StreamAdd` y `StreamAddAsync(clave, ...)` de StackExchange.Redis y los métodos de envío de un `ServiceBusSender` (`SendMessageAsync`, `SendMessagesAsync`, `ScheduleMessageAsync`, `ScheduleMessagesAsync`), cuya cola o tópico es el argumento del `CreateSender` que lo creó. Consumen `Subscribe(tópico)` y `Subscribe(tópicos)`, `BasicConsume(cola, ...)`, `ReceiveMessageAsync`, `SubscribeAsync(asunto, handler)`, `StreamReadGroup` y `StreamReadGroupAsync(clave, ...)` y `CreateProcessor(cola)` o `CreateProcessor(tópico, suscripción)`. `QueueBind(cola, exchange, clave)` vincula una cola con las reglas de enrutamiento de RabbitMQ, y un vínculo con comodín de NATS es `inferred`. Azure Service Bus no tiene comodín, y un procesador sobre un tópico consume todo mensaje enviado al tópico, sea cual sea la suscripción. El handler de un callback es el propio callback (una lambda o un método del repositorio pasado a `SubscribeAsync`) o cada handler asignado con `+=` al evento `Received`, `ReceivedAsync`, `MessageHandler` o `ProcessMessageAsync` del consumidor o del procesador que guarda el resultado de la llamada (en un local, en el mismo método; en un campo, en cualquier método de la clase), un consumidor por handler; el de un cliente que hace polling es el método con la llamada que nombra el tópico o la cola. `.ConfigureAwait(..)` en la cadena que crea el cliente no la interrumpe. Los nombres se evalúan como las URLs (constantes, campos `static readonly`, locales ligados una vez, interpolación y los argumentos de un wrapper en cada llamador, también el del método que crea un `ServiceBusSender` con `CreateSender(cola)`). Un `ServiceBusSender` escrito más de una vez (un campo asignado en dos constructores o en un setter, o un local rehecho) no tiene un nombre único y queda sin resolver. Un nombre que no se evalúa, incluida una lectura de variable de entorno o de `IConfiguration` (aquí no se lee `appsettings.json`), es un lado sin resolver. MassTransit, `BasicGet`, `SubscribeSync` y `QueueSubscribe`, NATS.Net v2 no se leen.

## Paquetes npm

Cada `package.json` con nombre genera `package::npm::<nombre>`. Entradas en `dependencies`, `devDependencies`, `peerDependencies` y `optionalDependencies` se vinculan con paquetes del workspace, incluidos los del mismo repositorio. Se compara el rango con la versión local: `range mismatch` si no la acepta, `range unchecked` si no puede comprobarse.

Las llamadas a funciones importadas llegan al código del proveedor mediante `exports`, `types`, `module`, `main` y re-exports. Si la entrada apunta al build, el `outDir` del `tsconfig.json` se mapea de vuelta al `rootDir`. `search` con tipo `package` muestra nombre, repositorio, carpeta, versión y consumidores. `context` de una función exportada lista sus llamadores en todo el workspace.

## Paquetes Python

Cada `pyproject.toml` con nombre en `[project]` o `[tool.poetry]` genera `package::pypi::<nombre>`, normalizado según PEP 503. Dependencias en `[project] dependencies`, `[tool.poetry.dependencies]` y `requirements*.txt` se vinculan con paquetes del workspace. El rango queda como está escrito y se compara con la versión local como manda PEP 440: `range mismatch` si los especificadores no la aceptan, nada más si la aceptan, `range unchecked` si el proveedor no tiene versión o un especificador no puede leerse. Se comprueban `==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`, `==X.*` y `===`, separados por comas; la sintaxis de Poetry (`^1.2`, `~1.2`, `*`) y los marcadores de entorno no se comprueban. Una versión preliminar se rechaza salvo que un especificador nombre una. Un mismatch no elimina ningún candidato.

El `[tool.uv.sources]` de un proyecto uv elige el paquete que instala una entrada de `[project] dependencies`, como las fuentes de Cargo: `{ workspace = true }` el miembro del workspace uv del consumidor (el `pyproject.toml` más cercano, en él o por encima, con `[tool.uv.workspace]` cuyos globs de `members`, menos `exclude`, lo incluyen), `{ path = "..." }` el paquete de ese directorio y `{ git = "..." }` el repositorio del workspace de esa URL, cada uno como `exact`, para la entrada y para las llamadas por sus imports. Los demás paquetes del mismo nombre no reciben vínculo. Una fuente `url` o `index` queda fuera. Una lista de fuentes (una por marker), y una fuente workspace cuya raíz o cuyo único miembro con el nombre no se encuentra, siguen la regla anterior. Un paquete del propio repositorio del consumidor que ninguna fuente uv nombra (una entrada de `[project] dependencies` o de `requirements*.txt` sin fuente) solo es `inferred` cuando es el único paquete del nombre, con el motivo "no uv source names the local package, the registry package is the likely provider": pip y uv instalan el paquete del registro, así que un stub homónimo en el repositorio es un candidato, no una certeza. Un único proveedor en otro repositorio del workspace sigue `exact`.

Un import llega al proveedor como `exact` si el módulo tiene el nombre de la distribución, `inferred` si es un módulo de nivel superior de ella y `ambiguous` si varias distribuciones lo exponen. Paquetes npm y PyPI con el mismo nombre no se vinculan.

## Paquetes Rust

Cada `Cargo.toml` con `[package] name` genera `package::cargo::<nombre>`. Cada miembro de un workspace Cargo es un proveedor. Entradas en `[dependencies]`, `[dev-dependencies]` y `[build-dependencies]` se resuelven así:

- `path`: crate de esa carpeta, en el mismo repositorio o en otro del workspace. Se resuelve por texto desde las raíces, sin leer fuera de ellas ni seguir enlaces simbólicos. Una ruta fuera del workspace no aparece.
- `git`: repositorio identificado por la URL normalizada.
- Versión: proveedor del mismo nombre en el workspace.

`package =` da el nombre real de una dependencia renombrada. `workspace = true` lee `[workspace.dependencies]`; `version.workspace = true` lee `[workspace.package]`.

El vínculo es `exact`, con el rango escrito y `range unchecked`, porque no se comprueban rangos Cargo. Un `use` sigue `[lib] name` o la clave de la dependencia renombrada y los `pub use` hasta los elementos públicos. El vínculo también es `exact`, con `; import crate name` en la razón. Dependencias fuera del workspace no aparecen.

## Módulos Go

Cada `go.mod` con `module` genera `package::go::<ruta del módulo>`. `require` y `replace` a carpetas locales se vinculan con el proveedor en el workspace. Un `replace` que sale del repositorio se resuelve por texto, como `path` de Cargo; un `replace` por otro módulo (`replace a => github.com/owner/repo`) conserva, entre los proveedores del módulo requerido, el del repositorio del workspace cuya identidad es el inicio de esa ruta (se descarta un sufijo `/vN`; un subdirectorio indica la carpeta del `go.mod`), en `exact`, para la entrada y para las llamadas a través de sus imports. Un reemplazo que ningún repositorio del workspace contiene, o cuyo `go.mod` declara otra ruta de módulo, queda fuera.

El vínculo es `exact`, con la versión del `require` como rango, `(local none)` como versión local y `range unchecked`. Un import de `<módulo>/<carpeta>` llega a sus funciones, tipos y métodos exportados. Si existe la carpeta, el vínculo es `exact`, con `; import module path` en la razón.

Meada no lee `go.sum` ni lockfiles, incluido `Cargo.lock`. No consulta el proxy de módulos, el registro npm, PyPI ni crates.io.

## Paquetes Maven y Gradle

Cada `pom.xml` con coordenadas literales y cada proyecto Gradle con `group` y nombre literales genera `package::maven::<grupo>:<artefacto>`. Maven hereda el grupo literal de `<parent>` cuando hace falta; cada módulo es un artefacto. En Gradle, `rootProject.name` e `include` de `settings.gradle` o `settings.gradle.kts` nombran los proyectos de los archivos `build.gradle` o `build.gradle.kts`. Un proyecto renombrado o movido en `settings.gradle` (`project(":app").name`, `.projectDir`) y un `group` definido en `allprojects` no generan artefacto.

Las dependencias Maven y Gradle, también en notación de mapa (`group: 'g', name: 'a'`), se vinculan con artefactos del workspace con nivel `exact` y `range unchecked`. Una versión calculada (`${project.version}`, `$v`) mantiene el vínculo `exact`, con la versión tal como está escrita. Un grupo o artefacto que usa una propiedad o variable se resuelve cuando los archivos de build declaran el valor como literal (una entrada de `<properties>` del pom, `project.groupId` y `project.artifactId`, un pom padre del repositorio, un `val`, `def` o `ext` de Gradle asignado una sola vez a un string simple) y se vincula como uno escrito; si no, queda `ambiguous`, incluso con un solo candidato, y sus partes literales filtran candidatos, y una coordenada sin ninguna parte literal no genera dependencia. Una declaración Gradle sin coordenada, como `project(":tools")`, `files(...)`, `platform(...)` o una llamada, no genera dependencia. El `gradle.properties` del repositorio, el de la raíz del `settings.gradle` y el del módulo (el del módulo gana) también rellenan un `${nombre}` o `$nombre` de la coordenada y de la versión, siempre que el script no asigne el nombre; y un accesor de catálogo de versiones (`libs.models`, `libs.mi.lib` para `mi-lib` o `mi_lib`) pasa a ser la coordenada que la tabla `[libraries]` de `gradle/libs.versions.toml` (o del archivo `*.versions.toml` que un `from(files("..."))` de `settings.gradle` nombra con una ruta relativa) le da, con la versión de `[versions]`. Un alias que el catálogo no tiene no genera dependencia. `buildSrc`, los plugins de convención, los builds incluidos y los BOM que se resuelven al compilar no se leen. Los dos archivos se leen con un límite de tamaño, en una sola pasada. Las dependencias externas quedan fuera. Los lectores se comparten con Kotlin, sin ejecutar JDK, Maven o Gradle ni consultar registros.

Un `import` de una clase de un artefacto del workspace del que depende el repositorio llega a la clase en el proveedor mediante la declaración `package` de los archivos, no por una ruta: `context` sobre un método público lista las llamadas de otros repositorios que llegan a él, y sobre una clase pública las que llegan a sus constructores y métodos, una línea por llamada, y `trace` cruza el salto. Cubre llamadas `Tipo.metodo()`, `new Tipo(...)`, llamadas sobre una variable, campo o parámetro del tipo importado, `import a.b.*` e imports estáticos. El método se elige por nombre y número de argumentos entre las sobrecargas de la clase y de las superclases e interfaces que tiene en el proveedor, como Java las combina. El vínculo es `exact` cuando un solo proveedor declara el paquete y el tipo, `ambiguous` cuando varios lo declaran o cuando quedan sobrecargas con el mismo número de argumentos, e `inferred` cuando un ancestro de la clase viene de fuera del repositorio del proveedor, porque puede declarar una sobrecarga mejor; la razón termina en `; import package`. Dos imports `import a.*` que traen una clase con el mismo nombre no generan vínculo. Las raíces de código son `src/main/java` y `src/test/java` de cada módulo Maven y los directorios literales de `sourceSets { ... java { srcDirs = [...] } }` de Gradle (`srcDir`, `srcDirs +=` y la forma Kotlin DSL también). Las clases de una raíz de pruebas, o de un conjunto distinto de `main`, nunca las importa un dependiente; un archivo fuera de toda raíz sigue resolviendo por su declaración `package`. Los directorios calculados, y los dentro de `allprojects`, `subprojects`, `project(':x')`, `configure(...)` o un bloque condicional, no se leen. Un módulo del mismo repositorio del que depende el archivo pasa sus propias dependencias `compile` que no son `optional`, como hace Maven; las de alcance `test`, `provided`, `runtime` o `system` se quedan con él, y una dependencia de Gradle no se sigue.

En Kotlin, un `import` de una función de nivel superior, una función de extensión o una clase de ese artefacto llega a la declaración del mismo modo: `greet("x")` elige la función del paquete importado por nombre y cantidad de argumentos, `Money(5)` llega a la clase, `Money.of(5)` a su `companion` sin nombre y `order.format()` a una función de extensión de la clase importada. Una función de extensión solo se sigue por el `import` de su nombre (o de un `import` con `*` de su paquete) y queda `exact` cuando el tipo escrito del receptor es una clase del artefacto, cuyos miembros se conocen todos, y una sola extensión de ese tipo cabe en la llamada; en un receptor de tipo desconocido o de fuera del workspace queda `inferred`, porque un miembro de ese tipo puede ocultarla. Un miembro del tipo del receptor con el mismo nombre prevalece sobre ella, y una que el propio repositorio declara no se confunde con ella. Una llamada que puede dejar fuera un argumento con valor por defecto, o que llena un `vararg`, llega a todas las sobrecargas que podría, `ambiguous` cuando hay más de una; una declaración `internal` o `private`, y una bajo una raíz de pruebas no se siguen. Las raíces de fuentes incluyen también `src/main/kotlin` y `src/test/kotlin`, y los directorios de `sourceSets` de Gradle se leen para `kotlin { srcDirs = [...] }`, `srcDir("...")`, `sourceSets["main"]` y `sourceSets.getByName("test")` como para `java`; un conjunto Kotlin es de pruebas cuando se llama `test` o termina en `Test`, y de producción cuando se llama `main` o termina en `Main`, y cualquier otro no se lee. Un `companion` con nombre (`companion object Factory`) y `setSrcDirs` no se siguen.

## Paquetes NuGet

Cada `.csproj` con SDK provee `package::nuget::<id>`, donde el id es su `PackageId`, si no su `AssemblyName`, si no el nombre del archivo del proyecto. El `PackageId`, el `AssemblyName` y la versión que escribe un `Directory.Build.props` valen para los proyectos que tiene debajo, y los del propio `.csproj` ganan. Un id calculado (`$(Id)`), en el proyecto o en el `props`, no provee paquete. Un `PackageReference` que nombra un paquete del workspace enlaza con él en `exact`, con el atributo (o elemento) `Version` como rango y `range unchecked`. Un `ProjectReference` a un archivo de proyecto de otro repositorio del workspace enlaza con el paquete de ese proyecto en `exact`, diga lo que diga el nombre del archivo de su id; el que apunta a un proyecto del propio repositorio se queda dentro. Una dependencia fuera del workspace queda fuera, y Meada no consulta ningún feed ni lee `Directory.Build.targets` ni `Directory.Packages.props`. Los ids se comparan sin distinguir mayúsculas de minúsculas, como hace NuGet; la ruta de un `ProjectReference` se compara con las mayúsculas escritas.

Un `using` de un namespace de un proyecto del que depende el archivo alcanza sus tipos por namespace: `context` sobre un método o tipo público lista las llamadas de otros repositorios que lo alcanzan, y `trace` cruza el salto. Cubre `Tipo.Metodo()`, `new Tipo(...)`, llamadas sobre una variable, campo, propiedad o parámetro del tipo, aunque ninguna llamada del archivo nombre el tipo, usings de namespace y `using static`. El método se elige por nombre y número de argumentos. El enlace es `exact` cuando un proveedor declara el namespace y el tipo, `ambiguous` cuando varios lo declaran e `inferred` cuando un ancestro de la clase viene de fuera del repositorio del proveedor; la razón termina con `; import namespace`. Los métodos de extensión, un local `var` que guarda el resultado de una llamada y un nombre totalmente cualificado escrito sin `using` no se siguen. El archivo también ve lo que pasan los proyectos de su propio repositorio a los que su proyecto hace referencia, directa o indirectamente; ante un nombre repetido, gana la referencia del proyecto más cercano. Una referencia cuyo `PrivateAssets` incluye `all` o `compile`, o es calculado (`$(...)`), no se pasa.

## Paquetes Swift

Cada `Package.swift` provee `package::swiftpm::<nombre>`, donde el nombre es el literal de `Package(name:)`; un nombre calculado provee el paquete nombrado por su patrón, con `*` en lugar de cada parte calculada (`Kit*`). Los productos `.library(name:targets:)` y los targets de cada uno quedan registrados, y solo los módulos de un producto de biblioteca se pueden importar. Un `.package(url:)` enlaza con el repositorio del workspace cuya identidad de remoto (ADR 0004) es la de la URL, y un `.package(path:)` con el repositorio en esa ruta, en `exact`, con el requisito escrito (`from`, `exact`, `branch`, `revision`, `.upToNextMajor`, rangos) como rango, `(local none)` y `range unchecked`. Solo cuentan los valores literales: una URL, ruta o nombre calculado (`"\(vendor)"`) vuelve el enlace `ambiguous`, y lista a todo proveedor que encaja con las partes literales; lo mismo vale para un paquete cuyo `Package(name:)` es calculado. Una dependencia fuera del workspace se deja fuera, y Meada no lee `Package.resolved` ni usa la red.

Un `import Modulo` que nombra un módulo de un paquete del que depende el `Package.swift` alcanza sus declaraciones exportadas: `Modulo.f(a:)`, `f(a:)`, `Tipo(a:)` y `valor.metodo(b:)` sobre una variable, parámetro o constante local de tipo escrito o recién construida. Las etiquetas de los argumentos eligen entre las sobrecargas, y un parámetro con valor por defecto puede omitirse. Solo se siguen las declaraciones `public` u `open`. El enlace es `exact` cuando un único paquete expone el módulo y `ambiguous` cuando varios paquetes de los que depende el archivo exponen el módulo, aunque solo uno declare el nombre; la razón termina en `; import module`. Los módulos de la plataforma (`Foundation`, `Dispatch`, `SwiftUI` y otros) nunca hacen enlace. Todas las dependencias del `Package.swift` más cercano valen para los archivos debajo de él, sin distinguir lo que cada target declara con `.product(name:package:)`, y un miembro `internal` de un tipo público, un miembro heredado y una extensión de otro módulo no se distinguen.

## Drift de formato

Drift es un campo que un consumidor `fetch` lee de una respuesta pero que la ruta no devuelve. Meada compara la lectura con OpenAPI, un schema zod o los objetos literales del handler.

| Consulta | Resultado |
| --- | --- |
| `trace` | Añade `drift: missing <ruta>` al vínculo |
| `context` de una ruta | Muestra origen del formato, consumidores, drift, nivel y lecturas no comprobadas en partes abiertas |
| `search` con tipo `drift` | Lista el drift encontrado |
| `meada status` | Cuenta lecturas ausentes en el repositorio consumidor; el proveedor no recibe esa cuenta |

## Valores de configuración

Cada escalar de cada archivo `*.yaml`, `*.yml` y `*.json` que Meada lee, y de los archivos YAML y JSON de la configuración de CI en la raíz (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`, `.buildkite/`, `.drone.yml`), es un valor de configuración: archivo, ruta de la clave (al estilo de jq, `alerts.loki[0].labels.support`, `metadata.labels["app.kubernetes.io/name"]`, `[2].spec.replicas` en un archivo de varios documentos), valor, tipo (texto, número o booleano) y línea. Las demás rutas ocultas quedan fuera, y no hay nada que activar.

Los archivos Terraform y OpenTofu también son valores de configuración (`*.tf`, `*.tofu`, `*.tf.json`, `*.tofu.json`, `*.tfvars`, `*.auto.tfvars`, `*.tfvars.json`). La ruta de la clave pasa por el tipo y las etiquetas del bloque y por el nombre del atributo (`resource.aws_sqs_queue.orders.name`, `resource.opsgenie_team_routing_rule.critical.criteria.conditions[1].expected_value`), y un bloque repetido con el mismo tipo y las mismas etiquetas se indexa `[n]` (`ingress[0]`). Un literal conserva su tipo; una expresión (`var.environment`, `format(..)`) o una plantilla (`"${var.environment}-orders"`, un heredoc con interpolación) se guarda tal como está escrita, con el tipo `expression` o `template`, hasta que la evaluación estática la resuelva. Un archivo `.tofu` oculta el archivo `.tf` del mismo nombre en su carpeta (el `.tofu.json`, el `.tf.json`), como hace OpenTofu, y el archivo oculto no se lee. Junto a un archivo de override, cuenta como un archivo que no se lee, porque Terraform, que ignora los archivos `.tofu`, lo lee.

| Consulta | Resultado |
| --- | --- |
| `search` con kind `value` | Valores cuyo valor o ruta de la clave es igual a la consulta, empieza con ella o la contiene, como `<repositorio> <ruta>:<línea> <ruta de la clave> = <valor>` |
| `search` sin kind | Valores solo cuando ningún símbolo y ningún contrato coincide |
| `meada status` | Archivos de configuración leídos y omitidos (lockfiles, archivos que no se leen) y los valores ocultados |

Un valor de más de 1 KiB se guarda cortado, marcado `(cut)`. Los `data` y `stringData` de un `Secret` de Kubernetes se guardan como `redacted`, con ruta de la clave, tipo y línea, igual que el `default` de una `variable` de Terraform con `sensitive = true` (o `"true"`, que Terraform convierte), también cuando otro archivo de la misma carpeta la marca así y la carpeta tiene un archivo de override (`override.tf`, `*_override.tf`), que Terraform fusiona con los demás, y todo valor que un archivo `.tfvars` da a una variable de ese nombre (todos, cuando un archivo Terraform en la misma carpeta o por encima no se lee) y el `value` de un recurso `aws_ssm_parameter` con `type = "SecureString"` literal, en HCL o Terraform JSON; nada más se filtra, así que los archivos que no deben entrar en el índice van en `.meadaignore`. Los lockfiles (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `packages.lock.json`), las plantillas Helm (que no son YAML hasta renderizarse), los archivos de más de 1 MiB y los archivos de más de 1.000 documentos quedan fuera. El texto de los valores nunca entra en `meada export`.

Un ARN AWS reconocido por la tabla de tipos, una URL SQS o `s3://archive/ruta` en configuración se convierte en un lado de uso `exact`, ubicado por `<archivo>#<ruta de la clave>`. Un nombre simple bajo la última clave `queue`, `topic`, `bucket` o `table` se convierte en un uso SQS, SNS, S3 o DynamoDB `inferred`, respectivamente. Por ejemplo, `queue: orders` en valores Helm usa `channel::sqs::orders`; `other: orders` no enlaza nada. El uso no tiene dirección: solo enlaza a los provisionadores y cuenta para huérfanos, sin convertirse en productor, consumidor, escritor o lector. `context` y la búsqueda de contratos muestran estos usos, y `trace` sigue los enlaces de aprovisionamiento hasta la clave y de vuelta. Los valores ocultados, cortados o sin resolver quedan fuera; el atributo que define el nombre de un provisionador no usa su propio recurso. El export incluye los lados en `uses`, sin el texto de los valores, y la consola los muestra separados de los lados con dirección.

## Nodos IaC

Cada `resource`, bloque `data` y llamada `module` de los archivos `*.tf`, `*.tf.json`, `*.tofu` y `*.tofu.json` es un nodo IaC, con dirección (`<tipo>.<nombre>`, `data.<tipo>.<nombre>`, `module.<nombre>`), tipo, archivo, líneas y el módulo raíz que empieza su evaluación: una carpeta que ninguna otra llama como módulo local, etiquetada por su ruta (`.` en la raíz). Cada llamada de un módulo local (un `source` `./` o `../` que queda dentro del repositorio) instancia sus bloques con la dirección de la llamada delante (`module.orders.aws_sqs_queue.this`), y un `count` o un `for_each` sobre valores que se resuelven en literales da un nodo por instancia, con `[clave]` al final (`aws_sqs_queue.dlq[0]`). Una expresión que nombra un resource, un bloque data o un módulo es una referencia, guardada como una llamada; una que pasa por `var.<nombre>`, `local.<nombre>` o la salida de un módulo se sigue hasta lo que nombra, y guarda el nombre por el que pasó.

El JSON Terraform usa las mismas reglas de evaluación y referencia. Las cadenas comunes son plantillas; una interpolación sola conserva el tipo del resultado, por lo que `${var.items}` puede proporcionar una colección. Los defaults de variables y las fuentes de módulos son literales. Las cadenas estáticas de `depends_on` nombran referencias. Los arrays de bloques conservan sus índices en las rutas de configuración. Las dos extensiones JSON admiten direcciones de nodos en las ubicaciones `via` de recursos declarados.

| Consulta | Resultado |
| --- | --- |
| `search` con kind `iac` | Nodos cuya dirección, tipo o nombre físico (el argumento `name` como la evaluación estática lo resuelve) es igual a la consulta, empieza con ella o la contiene, como `<repositorio> <ruta>:<línea> <dirección> (<tipo>, name "<nombre>", root <módulo raíz>)`, `pattern` en lugar de `name` cuando una parte no se resuelve |
| `search` sin kind | Ningún nodo IaC: solo los valores de configuración que guardan su nombre, cuando nada más coincide |
| `context` en una dirección | Tipo, nombre físico (`(pattern)` cuando una parte no se resuelve), módulo raíz, las variables de la raíz por las que pasó el nombre (`overridable:`, `(default)` o `(tfvars)`), las referencias en los dos sentidos (`references:`, `referenced by:`, cada una con su `via`) y los valores de configuración del bloque |
| `trace` desde una dirección | Las referencias, `down` (lo que el nodo alcanza) o `up` (lo que lo alcanza), dentro del módulo raíz y por los módulos locales |

Toda referencia es `exact`: un nombre tal como está escrito, seguido sin evaluar valores. La
evaluación estática resuelve el nombre físico de cada nodo y cada `count` y `for_each`, sin ejecutar
Terraform ni acceder a la red, a partir de los valores del propio repositorio: los defaults de las
variables, el `terraform.tfvars`, el `terraform.tfvars.json`, los `*.auto.tfvars` y los
`*.auto.tfvars.json` de la raíz (en ese orden, ganando el archivo siguiente), los locals, las
entradas y salidas de un módulo local por llamada, `each`, `count.index`, plantillas y las funciones
puras de texto, colección y conversión del ADR 0025 con `jsonencode`. Los archivos de override
(`override.tf`, `*_override.tf`) se fusionan con los bloques de su carpeta después de los demás
archivos de ella, como Terraform los aplica: lo que un bloque de override define reemplaza lo del
bloque base, referencias incluidas, y junto a uno, un archivo Terraform que no se lee deja sin
evaluar todas las variables de la carpeta, porque puede marcar cualquiera como sensible. Data
sources, atributos de
resources, módulos del registro, `terraform.workspace`, `-var`, `-var-file`, `TF_VAR_`, variables
sensibles y cualquier otra función nunca se evalúan: esa parte es un `*` y el nombre un patrón
(`orders-*`), que enlaza nombres literales según las reglas de patrones de abajo. Un nombre que pasó por una variable del módulo raíz, por
su default o por un archivo de variables, la registra, ya que quien despliega puede cambiarla. Un
valor de más de 64 KiB, una expresión cuya evaluación podría pasar de 1 MiB y lo que una raíz evalúa
más allá de 64 MiB quedan sin resolver. Un archivo Terraform o de variables cambiado evalúa de nuevo
solo los módulos raíz cuya unidad (la raíz y los módulos locales que llama) lo contiene. Un `count`
o un `for_each` que no se resuelve da un solo nodo, sin clave; un módulo del registro o remoto es
solo el nodo de la llamada. Solo los
nodos con un lado de contrato entran en `meada export`.

### Contratos huérfanos

`search` sin filtro, y con `kind = "contract"`, `channel` o `resource`, también encuentra los lados provisionadores por contrato.

`meada status` lista contratos huérfanos por tipo y broker (o tipo de recurso): provisionadores
sin uso y usos sin provisionador. Solo hay líneas de huérfanos cuando el repositorio o workspace
consultado contiene al menos un provisionador y un uso de ese grupo. Un repositorio solo de
código o solo de IaC no genera esas líneas; `search` y `context` marcan el propio lado con
`no provisioner in the workspace` o `no use in the workspace`. Los vínculos de aprovisionamiento,
incluidos patrones y módulos raíz independientes, determinan si hay contraparte. Los avisos
existentes de recursos declarados sin escritores y lectores siguen separados.

### Provisionadores Terraform

Un resource `aws_sqs_queue` provisiona `channel::sqs::<nombre>` por el argumento `name`;
un `aws_s3_bucket` provisiona `resource::s3::<nombre>` por `bucket`. La tabla de tipos en
`src/iac/types.toml`, integrada en el binario, también lee nombres en ARN y URL. Un
`[[resource]]` declarado con `kind = "s3"` y el mismo `id` participa en ese contrato,
conservando `declared`.

CloudFormation y SAM usan los mismos contratos para SQS, SNS, EventBridge, Kinesis, S3,
DynamoDB (Table, GlobalTable y SimpleTable), RDS (instancia o clúster), ElastiCache
(clúster o grupo), SSM y Secrets Manager cuando se conoce el nombre físico. La propiedad
que define ese nombre no usa su propio recurso; los secretos solo aportan su identidad.

Cada productor, consumidor, escritor y lector se enlaza con cada provisionador, identificado
por dirección y módulo raíz. Raíces como `envs/dev` y `envs/prod` no son ambiguas entre sí.
Nombres escritos dan `exact`; nombres que pasan por valores reemplazables dan `inferred`, con
las variables y sus orígenes en el motivo. `context` en el bloque lista los lados del contrato;
en un productor lista sus provisionadores. `trace` sigue del bloque al código con `down` y del
código a la infraestructura con `up`. El export conserva `format: 1` y añade `iac_nodes`,
`provisioners` y `provisions`, omitidos cuando están vacíos. Tipos no listados y bloques data no
provisionan contratos. Un patrón con al menos tres letras o dígitos literales (los separadores no
cuentan) y comodines en los límites de segmentos (`-`, `_`, `.`, `/`) se enlaza a un nombre literal
en `inferred`, o `ambiguous` cuando coinciden varios nombres. Los patrones nunca se enlazan entre
sí. El grafo de la consola muestra los provisionadores y sus enlaces propios.

### SDK AWS en Java, Go y C#

Las operaciones siguientes crean lados de contratos sin declaración en `meada.toml`. El cliente debe proceder de un constructor del SDK, con cada salto usado para demostrar ese origen resuelto mediante una llamada exacta. Parámetros, locales y nombres de tipos que ocultan el cliente o su constructor no heredan su origen. SQS conserva sus formas anteriores en Java, Go y C#: un campo, parámetro o local tipado que el código declara con el tipo de cliente del SDK (`SqsClient` o `SqsAsyncClient`, `*sqs.Client`, `IAmazonSQS` o `AmazonSQSClient`) es un cliente, también inyectado; un tipo del repositorio con ese nombre no lo es. Un nombre literal se une al nombre físico aprovisionado; una lectura del entorno sin una entrada correspondiente en el workload queda sin resolver y conserva `env:NOMBRE` en el export, incluidos los productores. Meada nunca lee el entorno del proceso.

Las lecturas del entorno mediante alias de importación en Go y C# conservan el nombre de la variable. Una solicitud guardada por nombre queda sin resolver cuando se escribe el campo que nombra el recurso, incluso mediante un alias.

| Servicio | Operaciones y dirección |
| --- | --- |
| SQS | `SendMessage`, `SendMessageBatch` producen; `ReceiveMessage` consume, mediante `QueueUrl` |
| SNS | `Publish`, `PublishBatch` producen, mediante `TopicArn`; una publicación a un teléfono o endpoint no nombra ningún tema |
| EventBridge | `PutEvents` produce en cada `Entries[].EventBusName`, `default` si se omite |
| Kinesis | `PutRecord`, `PutRecords` producen, mediante `StreamName` |
| S3 | `PutObject`, `DeleteObject` escriben; `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` leen, mediante el bucket |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem` y las entradas `Put`, `Update` y `Delete` de `TransactWriteItems` escriben; `GetItem`, `Query`, `Scan`, `BatchGetItem`, las entradas `Get` de `TransactGetItems` y las entradas `ConditionCheck` de `TransactWriteItems` leen, mediante `TableName`, todas las claves de `RequestItems` o el `TableName` de cada entrada de `TransactItems` |
| SSM | `GetParameter`, `GetParameters` leen; `PutParameter`, `DeleteParameter`, `DeleteParameters` escriben, mediante `Name` o cada entrada de `Names` |
| Secrets Manager | `GetSecretValue`, `BatchGetSecretValue` leen; `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` escriben, mediante `SecretId`, cada entrada de `SecretIdList` o `Name` |

Los nombres de operaciones usan la grafía Go; Java empieza en minúscula y .NET acepta formas síncronas y `Async`. S3 en .NET usa `BucketName` y `GetObjectMetadata` para `HeadObject`. Los ARN se reducen al nombre físico de la infraestructura. `GetParametersByPath` queda sin resolver porque `Path` es una jerarquía, no un parámetro. Las operaciones de control de RDS y ElastiCache no demuestran lecturas o escrituras de datos. Un recurso sin nombre nunca enlaza; una declaración de `meada.toml` conserva el nivel `declared`.

### GCP y Cloud Run

Terraform/OpenTofu reconoce `google_pubsub_topic` y `google_pubsub_subscription` como canales
`pubsub`, `google_storage_bucket` como `gcs`, `google_sql_database_instance` y
`google_sql_database` como `cloudsql`, y `google_secret_manager_secret` como
`gcpsecretmanager`. El nombre viene de `name`, excepto los secretos, que usan `secret_id`.
Las instancias Cloud SQL usan `<instancia>`; las bases usan `<instancia>/<base>`, con una
instancia literal o una referencia resuelta a una instancia conocida. Una base no aprovisiona
su instancia. Los proyectos quedan fuera de la identidad.

Un tópico usa `channel::pubsub::<id>`; una suscripción usa
`channel::pubsub::subscriptions/<id>`, incluso cuando ambos comparten el mismo id. La
suscripción enlaza a sus consumidores con los productores del tópico indicado por `topic`:
`exact` sin filtro, `inferred` con filtro o nombres sobrescribibles. Una fuente sin resolver
no crea un enlace ni inventa un tópico a partir del nombre de la suscripción.

`gs://<bucket>/objeto`, `projects/<proyecto>/topics/<id>`,
`projects/<proyecto>/subscriptions/<id>` y `projects/<proyecto>/secrets/<id>` (también con
`/versions/<versión>`) son usos de configuración `exact`. Los nombres simples bajo las claves
finales `pubsub_topic`, `pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`,
`cloudsql_database` y `gcpsecretmanager_secret` son `inferred`; `cloudsql_database` exige
`<instancia>/<base>`. Las claves genéricas `topic` y `bucket` conservan sus significados SNS y
S3. Los identificadores malformados, valores ocultados o cortados y la mera igualdad de texto
no crean usos.

`google_cloud_run_service`, `google_cloud_run_v2_service` y `google_cloud_run_v2_job` son
workloads: cada contenedor conserva su imagen y su entorno. Las imágenes enlazan al
repositorio que las publica; la evidencia sobrescribible debilita el enlace. Las lecturas de
entorno solo reciben valores del contenedor que ejecuta su código y quedan `inferred`. Las
referencias Secret Manager siguen como destinos sin resolver, sin leer el contenido del
secreto. Búsqueda, `context`, `trace` en ambos sentidos, huérfanos por ámbito y export incluyen
los nuevos contratos y runners; las entradas de entorno y valores de configuración quedan
fuera del export.

### Clientes de los SDK de GCP

Los cinco grupos de SDK de GCP (JavaScript/TypeScript, Python, Java, Go y C#) reconocen las formas documentadas de publicación y recepción por suscripción Pub/Sub, lectura/escritura de Cloud Storage y acceso a Secret Manager. La [guía de usuario](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.es.md#uso-de-los-sdk-de-gcp-en-cinco-grupos-de-lenguajes) enumera las operaciones y los constructores exactos. Los temas y las suscripciones conservan identidades distintas, incluso al resolver el entorno de la carga; una lectura del entorno sola sigue sin resolver.

### Canales, datos, secretos y suscripciones AWS

La tabla Terraform/OpenTofu también reconoce los tipos siguientes. Los ARN se reducen al
nombre físico, incluidas las rutas de parámetros y secretos; se elimina el sufijo de seis
caracteres del ARN de Secrets Manager. Las mismas reglas de aprovisionamiento, `context`,
`trace` y export se aplican a todos estos contratos.

| Tipo Terraform | Nombre físico | Contrato |
| --- | --- | --- |
| `aws_sns_topic` | `name` | `channel::sns` |
| `aws_cloudwatch_event_bus` | `name` | `channel::eventbridge` |
| `aws_kinesis_stream` | `name` | `channel::kinesis` |
| `aws_dynamodb_table` | `name` | `resource::dynamodb` |
| `aws_db_instance` | `identifier` | `resource::rds` |
| `aws_rds_cluster` | `cluster_identifier` | `resource::rds` |
| `aws_elasticache_cluster` | `cluster_id` | `resource::elasticache` |
| `aws_elasticache_replication_group` | `replication_group_id` | `resource::elasticache` |
| `aws_ssm_parameter` | `name`, conservando `/` inicial | `resource::ssm` |
| `aws_secretsmanager_secret` | `name` | `resource::secretsmanager` |

Un parámetro SSM `SecureString` guarda `value` como `redacted` en HCL y JSON de Terraform/OpenTofu. Si `type` es una expresión que
el lector de valores no resuelve, su valor también se oculta por precaución.

`aws_sns_topic_subscription` con `protocol = "sqs"` enlaza consumidores de la cola con
productores del tópico. `aws_cloudwatch_event_rule` junto con `aws_cloudwatch_event_target`
hace lo mismo del bus a la cola; sin `event_bus_name` se usa `default`. Sin filtro el nivel es
`exact`; con `filter_policy` o `event_pattern` es `inferred`. Los nombres reemplazables también
dan `inferred`, con sus variables en el motivo. El motivo identifica la suscripción o el target
y su regla. Endpoints sin resolver, otros protocolos y targets sin una regla única en el mismo
bus no crean enlaces. Las reglas solo con programación no enrutan eventos publicados en el bus.

`context` muestra productores y consumidores a través de la suscripción. `trace` sube del
consumidor al productor y baja del aprovisionador a los consumidores; un canal de origen `sns`,
`eventbridge` o `kinesis` también puede iniciar el trace. Los resúmenes entran en el array opcional `subscriptions`, con origen, destino, nodo y raíz.
Los enlaces entran en `links`,
conservando aparte los de aprovisionamiento en `provisions`. `meada status` muestra contratos
aprovisionados sin uso y usos sin aprovisionador, por kind y broker, solo cuando el workspace
contiene al menos un aprovisionador y un uso del grupo. Los otros formatos IaC siguen como
nodos y referencias; esta tabla de contratos cubre archivos `.tf`, `.tf.json`, `.tofu` y `.tofu.json`.

## Kubernetes y Kustomize

Todo documento YAML o JSON con `apiVersion`, `kind` y `metadata.name` es un nodo IaC, con dirección `<Kind>[.<grupo>]/<namespace>/<nombre>` (`Deployment.apps/shop/api`, `Service/shop/api`; el grupo core no escribe grupo). Un kind de alcance de clúster no tiene namespace en la dirección (`ClusterRole.rbac.authorization.k8s.io/reader`), ni un objeto que no define ninguno y al que ningún overlay da uno. Una plantilla Helm (un nombre con `{{`) y un archivo de Kustomize no son objetos.

Una carpeta con `kustomization.yaml` que ningún otro kustomization nombra como base es un overlay y una unidad de evaluación. Sus `resources`, `bases` y `components` que nombran un archivo u otro kustomization dentro del repositorio se instancian bajo ella, cada nodo etiquetado por el overlay (`root overlays/dev`), con el `namePrefix`, el `nameSuffix`, el `namespace` y las `images` de cada kustomization en el camino aplicados, los de la base primero y los del overlay al final: una base usada por dos overlays es un nodo en cada uno, con su nombre y su namespace. Un `..` que queda dentro del repositorio se sigue (`../../base`). Una entrada remota (una URL, `github.com/org/repo//ruta?ref=v1`), por encima de la raíz del repositorio, absoluta, ausente, alcanzada por un symlink, en ciclo o anidada más allá de 16 niveles no se sigue, nunca se descarga, y `meada status` la cuenta (`entradas de kustomize no seguidas: N`). Los manifiestos que ningún kustomization lista, ni usa como patch, son simples: los manifiestos de una carpeta son una unidad de evaluación, etiquetada por la carpeta.

| Referencia | Guardada como |
| --- | --- |
| Un Service a los dueños de pod template (Deployment, StatefulSet, DaemonSet, Job, CronJob, Pod) cuyas etiquetas del pod contienen su selector | `via selector app=api` |
| Una carga de trabajo a los ConfigMaps y Secrets de `env`, `envFrom` y volúmenes | `via env DATABASE_URL`, `via envFrom`, `via volume settings` |
| Una carga de trabajo a su `serviceAccountName` | `via serviceAccountName` |
| Un Ingress a los Services de sus reglas y del backend por defecto | `via backend shop.example.com/api`, `via defaultBackend` |

Una referencia queda en un namespace y una unidad, y nombra el objeto como se escribió, antes del prefijo y del sufijo del overlay, que el overlay aplica después a los dos lados. Toda referencia es `exact`. `search` con kind `iac` y `context` responden por estos nodos como por los de Terraform, y `context` en una carga de trabajo añade `images:`, lo que cada contenedor ejecuta tras las `images` del overlay (`web: registry.example.com/shop/api:1.4.0`). Los patches, los generadores, `commonLabels`, las plantillas Helm y `kind: List` no se aplican ni se leen, y los objetos quedan fuera de `meada export`.

### Crossplane

Los objetos Crossplane son objetos Kubernetes, y `context` dice qué es cada uno para Crossplane en una línea `role:`: una definición de recurso compuesto (XRD), una composition (las dos sin namespace), un claim o un recurso compuesto (un objeto del kind de claim o compuesto que un XRD del repositorio define), un recurso gestionado (un objeto con `spec.forProvider`) o un recurso que crea una composition. Cada recurso que una composition crea a partir de una `base` literal, en `spec.resources` o en el `input.resources` de un paso de pipeline que ejecuta la función patch-and-transform, es un nodo `<composition>/<recurso>` del kind de la base, con el nombre de su anotación `crossplane.io/external-name` o de su `metadata.name`; el nombre de un recurso gestionado también es su external name.

| Referencia | Guardada como |
| --- | --- |
| Un claim al XRD de su kind compuesto | `via composite XPostgreSQLInstance.database.acme.io` |
| Un recurso compuesto a su XRD | `via kind XNetwork.network.acme.io` |
| Un claim o recurso compuesto a la composition que nombra su `compositionRef` | `via compositionRef` |
| Un XRD a su composition por defecto y a la impuesta | `via defaultCompositionRef`, `via enforcedCompositionRef` |
| Una composition al XRD de su `compositeTypeRef` | `via compositeTypeRef XPostgreSQLInstance.database.acme.io` |
| Una composition a cada recurso que crea | `via resources`, `via pipeline <paso>` |
| Un recurso creado o gestionado a la definición de su kind (un XRD o un `CustomResourceDefinition`) | `via kind Bucket.s3.aws.upbound.io` |

Toda referencia es `exact` y queda en una unidad, como las de Kubernetes: un claim en otra carpeta que su XRD conserva su papel y no referencia nada. `trace` desde un claim baja hasta su XRD, la composition y los recursos que crea. Los patches y las funciones no se evalúan: un nombre que un patch (o un patch set) define, o una base sin nombre, es el patrón `*`, y un paso que ejecuta cualquier otra función (Go templating, KCL) no crea nodo.

### Compose

Un archivo Compose (`compose.yaml`, `compose.yml`, `docker-compose.yaml` o `docker-compose.yml`) con su archivo de override en la misma carpeta es una unidad de evaluación, con raíz en la carpeta. Cada servicio es un nodo IaC `services.<nombre>` de tipo `service`. `depends_on`, `links` y las redes compartidas (`networks:`; un servicio sin ninguna está en `default`) son referencias, con `via depends_on`, `via links` o `via networks.<nombre>`. `${VAR:-valor}` se resuelve a su valor por defecto y `context` lo marca `(overridable)`; `${VAR}` queda en el texto como parte sin resolver, marcada `(unresolved)`. `context` muestra `image:` y `build:` del servicio, guardados para el contrato de imagen, con el Dockerfile que nombra el build (`dockerfile` o `dockerfile_inline`); un contexto de build solo se guarda como ruta dentro del repositorio (una ruta absoluta, una URL, una que sale de la raíz o una con una parte sin resolver no).

### CloudFormation y SAM

Una plantilla válida llamada `compose.yaml` conserva sus nodos y referencias en el índice inicial y las actualizaciones; el nombre del archivo no la convierte en Compose.

Una plantilla YAML (las etiquetas cortas `!Ref`, `!Sub`, `!GetAtt`, `!Join` y las demás se leen) o JSON con un mapa `Resources` que lleve `AWSTemplateFormatVersion` o la transformación SAM (`AWS::Serverless-2016-10-31`) es una unidad de evaluación, con raíz en su ruta. Cada recurso es un nodo IaC direccionado por su id lógico dentro de la plantilla, y su tipo (`AWS::SQS::Queue`, `AWS::Serverless::Function`: los tipos SAM son nodos como cualquier otro) es el tipo del nodo. `Ref`, `Fn::GetAtt` (con el atributo en el `via`: `via Fn::GetAtt.Arn`), las variables de `Fn::Sub` y `DependsOn` son referencias, todas `exact`. El nombre físico de cola, tema, bucket, tabla, función, stream, secreto y algunos tipos más se evalúa dentro de la plantilla: el valor por defecto de un parámetro (`context` lo lista en `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` y `Fn::Join`. Un pseudo parámetro (`AWS::Region`), un import (`Fn::ImportValue`), cualquier otra función y un parámetro sin valor por defecto quedan como partes sin resolver: el nombre pasa a ser un patrón, con un `*` por cada una. Un parámetro con `NoEcho` tiene su valor por defecto guardado como `redacted` entre los valores de configuración y ningún nombre pasa por él. Una plantilla modificada se evalúa de nuevo sola. Los stacks anidados, las macros y las condiciones no se siguen.

### Serverless Framework

Un `serverless.yml`, `serverless.yaml` o `serverless.json` con un `service` es una unidad de evaluación, con raíz en su ruta. Cada función es un nodo IaC `functions.<nombre>` de tipo `Serverless::Function`, con el nombre de su `name` o `<service>-<stage>-<función>` (la etapa sustituible por `--stage`, `opt:stage`), y su handler y sus eventos se guardan y los muestra `context` (`handler:`, `event:`: `http GET /orders`, `sqs Queue.Arn`, `schedule rate(5 minutes)`). Una función referencia, `exact`, los recursos que su cuerpo nombra con `Ref`, `Fn::GetAtt` o `Fn::Sub`. La sección `resources` se lee como CloudFormation: cada recurso es un nodo en su id lógico, con los nombres y referencias descritos arriba. `${self:...}` se resuelve al valor en esa ruta del archivo, y el valor por defecto de `${opt:...}` se resuelve y se marca `(overridable)`, porque quien despliega lo pasa por la línea de comandos; `${env:...}`, `${ssm:...}`, `${sls:...}`, `${file(...)}` y las demás fuentes quedan como partes no resueltas: un `*` en el nombre, `${...}` en un texto guardado. Un valor que se nombra a sí mismo, directo o en ciclo, queda sin resolver, y `${self:provider.stage}` sin etapa en el archivo es `dev`, como en el nombre de una función. Un servicio modificado se evalúa de nuevo solo. `serverless.ts`, los plugins y los valores de otros archivos no se leen.

### GitHub Actions

Un archivo `.yml` o `.yaml` directamente en `.github/workflows/` que tenga `jobs` es un pipeline, y un `action.yml` o `action.yaml` que tenga `runs` es una action, en cualquier carpeta; con los dos nombres en la misma carpeta, la action es el `action.yml`, el archivo que el runner carga. Cada archivo es una unidad de evaluación, enraizada en su ruta, y solo se lee el primer documento YAML. El archivo es un nodo IaC de dirección `.` (tipo `workflow` o `action`), que una consulta nombra por la ruta del archivo. Cada job de CI es un nodo `jobs.<id>` de tipo `job` (`jobs["publish-prd"]` cuando el id no es un identificador simple), y cada paso de CI es un nodo `jobs.<id>.steps[<n>]` o `runs.steps[<n>]` de tipo `step`. Un job con `strategy.matrix` es un solo nodo. Las referencias quedan dentro del archivo: el pipeline referencia sus jobs, un job o una action compuesta referencia sus pasos, y un job referencia los jobs que su `needs` nombra (`via needs`).

`search` con kind `ci` encuentra esos nodos por la dirección, el tipo, el `name`, el id del job y el `id` o `uses` del paso, como `<repositorio> <ruta>:<línea> <dirección> (<tipo>, id "<id>", name "<nombre>", uses "<uses>")`; el kind `iac` y la búsqueda por defecto no los muestran. `context` muestra las líneas del nodo, sus referencias en los dos sentidos y los valores de configuración de su bloque, como los de `with` y `env` de un paso. Un paso también se nombra con un selector en lugar del índice (`jobs.build.steps[id=login]`, y también `name=` y `uses=`). El selector nunca es identidad: la dirección de un paso es posicional, y un paso insertado antes lo renumera.

Un job de CI cuyo `uses` nombra un pipeline reutilizable de otro repositorio del workspace (`<owner>/<repo>/.github/workflows/<archivo>@<ref>`) es consumidor del contrato de CI `ci::<identidad del repositorio>/<ruta>`, y el nodo de archivo de ese pipeline, aquel cuyo `on` tiene `workflow_call` (como texto, elemento de lista o clave), es el proveedor. Un `uses` no escribe host, así que el host es el del propio consumidor; owner y repositorio quedan en minúsculas y la ruta queda como está escrita. El vínculo es `exact`, y el motivo muestra el ref Git como está escrito y el commit en que se leyó el índice del proveedor, `@main (indexed 1a2b3c4)`: Meada nunca resuelve un ref, y añade `, ref mismatch` solo cuando el ref es un id de commit completo (40 o 64 dígitos hexadecimales) distinto de ese commit. Un contrato reúne todas las llamadas a un archivo, sean cuales sean los refs. `context` en el pipeline lista cada job de CI que lo llama, con repositorio, archivo, dirección, ref Git y las claves de `with` y de `secrets` que pasa (`secrets: inherit` como esa palabra), y `context` en un job de CI muestra el pipeline que llama. `meada export` trae los lados, los vínculos y, en el consumidor, los campos opcionales `ref`, `with` y `secrets`. `search` con kind `contract` lista esos contratos, y la búsqueda por defecto responde como antes. Un `uses` cuyo repositorio no está en el workspace (`actions/checkout`, un pipeline de otra organización) es externo: no tiene lado, así que nunca es huérfano ni elemento abierto. Una matriz es un solo consumidor, y una llamada de un host a otro no tiene vínculo.

Las otras formas de `uses` se leen como GitHub las resuelve. Un paso de CI cuyo `uses` nombra una action del workspace (`<owner>/<repo>[/<carpeta>]@<ref>`) es consumidor del contrato de CI de esa action: `ci::<identidad del repositorio>/<carpeta>`, o solo `ci::<identidad del repositorio>` para una action en la raíz, provisto por el nodo de archivo de la action. Una llamada local de un job de CI (`./.github/workflows/<archivo>`) y un `$/<ruta>` de un job de CI o de un paso de CI nombran el repositorio del archivo que los escribe: se vinculan en `exact` con el motivo `own repository` y sin ref, también en un repositorio sin `origin`. El `./<carpeta>` de un paso de CI se lee en el workspace del runner de CI, así que solo se vincula al propio repositorio en un pipeline que no es reutilizable, después de un `actions/checkout` del mismo job de CI que no nombra `repository` ni `path`. En un pipeline reutilizable, en una action compuesta, sin checkout antes de él o después de un checkout que nombra uno de los dos, queda sin resolver: un elemento abierto sin candidato, que `search` con kind `unresolved` y `context` en el paso de CI muestran con la causa. `docker://<imagen>` no tiene contrato de CI, y una ruta local que sale de la raíz del repositorio no tiene lado. `context` en una action lista cada paso de CI que la llama, con el ref Git y las claves de `with`.

`meada status` lista como huérfano el pipeline reutilizable o la action que nada en el alcance visible llama, con su contrato y su archivo, solo cuando ese alcance (el repositorio solo, o el workspace) tiene al menos un proveedor y un consumidor del contrato de CI. Un repositorio de actions y pipelines reutilizables indexado solo no imprime línea de huérfano, y ni un `uses` externo ni un paso de CI no resuelto abren el alcance. Una llamada cuyo repositorio está en el workspace y cuya ruta no nombra proveedor allí se marca en el lado del consumidor, ` (absent at the indexed commit)`: en el `context` del job de CI o del paso de CI, en el `context` del identificador del contrato y en `search` con kind `contract`, y como `"absent": true` en la fila del consumidor de `meada export`. Ausente no es roto: el archivo puede existir en el ref Git que la llamada nombra, que Meada no resuelve, así que la marca no es línea de huérfano ni elemento abierto. Un archivo de pipeline que está allí y no tiene `on.workflow_call` recibe ` (absent at the indexed commit: the file is no reusable pipeline there)`. Una carpeta que tiene un `Dockerfile` y ningún archivo de action es una action para GitHub que Meada no lee: la llamada a ella no tiene lado, así que no lleva marca.

En un pipeline o en una action, el lector de CI decide qué significa una clave. Un nombre simple bajo una última clave tipada (`queue`, `topic`, `bucket`, `table` y los alias de Azure y GCP) no da uso allí: `with: { bucket: x }` nombra un input de la action, no un bucket S3. Un identificador estructurado en un valor (un ARN, una URL de cola, `s3://`, `gs://`) sigue siendo un uso `exact`, y en `meada export` su `site` es el paso de CI que tiene la clave, el job de CI cuando ningún paso la tiene y el archivo en los demás casos, con el mismo nodo como `side` de su enlace de aprovisionamiento. Un ARN de rol en `with` no da uso. Un `${{ env.X }}` se resuelve donde un `env` literal del paso de CI, de su job de CI o del pipeline da `X`, el más cercano primero, así que `s3://${{ env.BUCKET }}/x` enlaza como el literal. El `env` más cercano que define `X` decide: si allí el valor no es un texto literal (un número, un booleano, una lista), la parte queda sin resolver y nunca se lee de un `env` más arriba. Cualquier otra expresión es una parte sin resolver: dentro de un nombre de una forma AWS (un ARN, una URL de cola, `s3://`) hace un patrón (`s3://acme-${{ vars.STAGE }}-releases/x` usa `resource::s3::acme-*-releases`, que enlaza en `inferred` con el único bucket que encaje); dentro de un nombre `gs://`, de una ruta de Pub/Sub o de una URL de Azure, y como el nombre entero, no da uso. `secrets.X` y `vars.X` no tienen valor en un repositorio: quedan como están escritos en el valor de configuración y nunca se emparejan por nombre entre repositorios. Todo escalar sigue siendo un valor de configuración, y `[[resource]]` con `via` en una clave de pipeline sigue siendo válido. Un archivo en `.github/` que no es pipeline, un `action.yml` sin `runs` y los archivos de otros sistemas de CI conservan la regla del nombre de la clave.

El script `run` de un paso de CI se lee con la tabla de comandos, sin evaluar shell. Lee un script bajo `shell: bash`, `sh` o ningún shell declarado (el del paso de CI, si no el `defaults.run.shell` del job de CI, si no el del pipeline), de hasta 1024 bytes y sin heredoc. Un comando es la primera palabra de una línea (una `\` al final une dos) o la que sigue a `&&` o `;`, también dentro de `if` y de bucles; el texto en `echo`, en una cadena o en un comentario no da lado, y lo que sigue a `||` o a una tubería no se lee hasta el fin de la línea. `pip install`, `pip3 install`, `python -m pip install`, `uv pip install`, `pipx install` y `uv tool install` hacen del paso de CI un consumidor de `package::pypi::<nombre>` para cada nombre que un repositorio del workspace provee, con el rango como está escrito en el motivo del enlace: el `context` del paquete lista el paso de CI entre sus dependientes, y el `context` del paso de CI muestra `consumes package::pypi::<nombre>` y el proveedor. El nivel es el de la regla del tipo paquete: `exact` para un único proveedor en otro repositorio, `inferred` cuando está en el repositorio del propio paso de CI. Una ruta, una URL, `-e`, el archivo de `-r` (que no se sigue) y el valor de cualquier opción no dan lado. Un `${{ env.X }}`, un `$X` o un `${X}` vale lo que da un `env` literal del paso de CI, del job de CI o del pipeline; cualquier otra expresión o variable (`secrets.X`, `vars.X`, `github.*`, `matrix.*`, `$(...)`, comillas invertidas) hace, en parte del nombre, un patrón (`inferred` al único paquete que encaja, `ambiguous` a varios) y, como nombre entero, en un workspace que provee un paquete PyPI, un consumidor no resuelto que muestra lo que está escrito: un ítem abierto con su causa y sin candidato. En el rango o en un extra no cambia nada: el nombre es literal y el rango queda como está escrito. Límites: una variable que el script asigna, `cd`, una función y un script llamado (`./deploy.sh`, `make`) no se siguen; un CI runner Windows usa `pwsh` por defecto y el script se lee como `bash`; un nombre que el workspace provee gana a un paquete público del mismo nombre.

Un paso de CI cuyo comando es un script de consola de un paquete PyPI del workspace inicia ese programa. La primera palabra de un comando se compara, byte a byte, con los nombres bajo `[project.scripts]` y `[tool.poetry.scripts]` cuya función es un punto de entrada de programa. El paso de CI es entonces un consumidor de `package::pypi::<paquete>`, con el comando como está escrito después de `run` (`run core-publish --to prod`), y el enlace sigue hasta la función que el script nombra. El nivel acompaña la evidencia de qué paquete corre: `exact` cuando un comando anterior del mismo job de CI, en una línea anterior o en un paso de CI anterior, instala el paquete por un nombre literal; `exact` cuando el comando nombra el paquete (`uvx <paquete>`, `pipx run <paquete>`), lo que corre el script con el nombre del propio paquete; `inferred` cuando nada lo instala y un solo paquete del workspace declara el nombre; `ambiguous`, con un enlace a cada uno, cuando varios lo declaran (solo a los que el job de CI instaló, cuando instaló alguno); ningún lado cuando ninguno lo declara. El enlace nunca es más fuerte que el enlace de paquete que continúa, así que un paquete del propio repositorio del paso de CI da `inferred`, y el motivo nombra el script y la evidencia después del motivo del paquete: `package::pypi::core-sdk none (local 1.0.0), range unchecked; console script core-publish, its package installed earlier in the CI job`. `uvx <paquete>` o `pipx run <paquete>` para un paquete sin script con su propio nombre hace del paso de CI un consumidor del paquete y no inicia programa. `trace` cruza el enlace en los dos sentidos: hacia arriba, desde el programa o desde una ruta que este llama, hasta el paso de CI, el job de CI, el pipeline y los jobs de CI que llaman a ese pipeline; hacia abajo, desde un job de CI hacia dentro del programa y de lo que este llama. `context` sobre el paso de CI muestra el programa como proveedor, y `context` sobre el paquete lista el paso de CI entre los dependientes. Límites: el enlace nombra la función del script, nunca el handler de un subcomando; una instalación en un paso de CI posterior, en otro job de CI, en un script llamado, por `-e .` o por `-r` no es evidencia; solo se leen scripts de consola de PyPI; una ruta hasta el script, `python -m <módulo>`, `uv run`, `poetry run`, un prefijo (`sudo`, `VAR=x`), `uvx --from <paquete> <script>` y `<paquete>@<versión>` no se reconocen; un nombre que más de ocho paquetes del workspace declaran no da lado; y un script del workspace gana a un builtin del shell o a una herramienta pública del mismo nombre, en `inferred` cuando nada instala el paquete.

`trace` sigue una cadena de llamadas de CI en los dos sentidos, entre repositorios y a través de pipelines reutilizables intermedios y de actions compuestas (un paso de CI llama a una action cuyo paso de CI llama a otra). Hacia arriba, desde un pipeline reutilizable, alcanza cada job de CI que lo llama, después el pipeline del archivo de ese job de CI y, cuando ese archivo también es reutilizable, los jobs de CI que lo llaman. Hacia abajo, desde un job de CI, alcanza el pipeline que llama, después los jobs de CI y los pasos de CI de ese pipeline y lo que ellos llaman. El identificador del contrato de CI, el nodo de archivo del pipeline (`<repositorio>::<ruta>::.`) y la ruta del archivo inician la misma cadena. Un pipeline que se alcanza a sí mismo termina en la marca `(cycle)` del recorrido, que solo entra en repositorios del workspace.

La consola y el modo servidor muestran el tipo como la CLI y el MCP por stdio. El grafo de la consola tiene la vista **Enlaces de CI**: los contratos de CI con sus lados y el enlace de cada llamada, con el ref Git en el motivo y la marca de ausente, y el playground acepta el kind `ci`. `context` en un pipeline, una action, un job de CI o un paso de CI lista los lados encima de los valores, así que un pipeline largo muestra quién lo llama dentro del presupuesto por defecto. En el modo servidor (`meada mcp --listen`), el MCP sobre HTTP responde a cada consulta del tipo como el stdio sobre los mismos checkouts, salvo por el commit indexado: es el que `meada index --workspace` registró, y ese comando no registra ninguno para un checkout que no está limpio (`git status` lista un archivo, incluso ignorado), de modo que un enlace con ese proveedor dice `@<ref> (indexed commit unknown)` y nunca lleva `ref mismatch`, donde el stdio nombra el `HEAD` del checkout. Los lados siguen el manifiesto, el grant, la generación y el workspace con nombre: el que no incluye el repositorio del proveedor no muestra enlace con él ni consumidor de él, en `context`, en `trace`, en el grafo y en el export.

## Comunidades y procesos

Una comunidad agrupa símbolos que se llaman más entre sí que al resto. `meada export` calcula los grupos con Leiden, sin aleatoriedad y con aritmética entera. El nombre viene de la carpeta de la mayoría de sus miembros (`src`, `src#2`), igual en cada máquina.

Cada vínculo tiene nivel `exact`, `declared`, `inferred`, `decided` o `ambiguous`. Una URL sin resolver solo se convierte en vínculo tras una decisión, con nivel `decided`. Una ruta cuyo lugar de servicio se desconoce queda fuera del mapa.

Un proceso es el árbol de llamadas desde un punto de entrada, entre repositorios. Son puntos de entrada las rutas, los handlers de canales y las funciones o métodos exportados sin llamadores en su propio repositorio. También se incluyen:

- Python: `if __name__ == "__main__":` (símbolo `__main__`) y funciones de `[project.scripts]` en formato `módulo:función`. Son exportados los nombres públicos, sin `_` inicial, o los listados en un `__all__` literal.
- Rust: `fn main` de cada binario en `src/main.rs`, `src/bin/` o `[[bin]] path`.
- Go: `func main` en un `package main`.
- Swift: un tipo marcado `@main` (por su método `main()` cuando declara uno) y el código de nivel superior de un archivo `main.swift`, en un símbolo `main` dueño de las llamadas escritas allí.

`trace` con `process::<identidad del punto de entrada>` lee el árbol. `meada export` incluye cada proceso. `search` con tipo `community` encuentra comunidades por nombre; con tipo `process`, encuentra procesos por la etiqueta de su punto de entrada; con tipo `duplicate`, lista los símbolos cuyo texto es idéntico en dos o más lugares del workspace (entre repositorios o dentro de uno), en grupos ordenados por el tamaño del tramo, encontrados por el nombre o la ruta de un miembro. El export trae el `hash` BLAKE3 de cada símbolo; las copias casi iguales (una diferencia de espacio en blanco) no son duplicados. `context` muestra la comunidad del símbolo (o `community: none`) y sus procesos, recorriendo llamadas y vínculos hacia los llamadores.

## Límites y próximos pasos

Los lenguajes y clientes HTTP o de mensajes fuera de estas listas aún no tienen soporte. El [ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md) define el alcance y el orden. Consulta el [roadmap](roadmap.md), las [decisiones](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) y el [CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).

## Llamadas del AWS SDK en código

Las lecturas de recursos conservan cada llamada SDK como lado distinto en JS, TS y Python. `trace` hacia arriba llega a sus escritores; desde un escritor, hacia abajo llega a la función lectora y continúa por sus llamadas, con la ubicación real de cada operación.

TypeScript y JavaScript reconocen comandos v3 enviados con `cliente.send(new Comando(...))`,
con cliente y comando importados del paquete AWS correspondiente. Python reconoce métodos de
clientes creados por `boto3.client(...)` o una `Session`. Los nombres pueden venir de constantes
y argumentos de wrappers resueltos exactamente. Un cliente alcanzado solo por coincidencia
de nombre, u oculto por un parámetro, `catch`, desestructuración u otro binding, no aporta lado.

| Servicio | Produce o escribe | Lee | Argumento del nombre |
| --- | --- | --- | --- |
| SNS | `Publish`, `PublishBatch` | | `TopicArn` |
| EventBridge | `PutEvents` | | `Entries[].EventBusName`, `default` si se omite |
| Kinesis | `PutRecord`, `PutRecords` | | `StreamARN` o `StreamName` |
| S3 | `PutObject`, `DeleteObject` | `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` | `Bucket` |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, `TransactWriteItems` | `GetItem`, `Query`, `Scan`, `BatchGetItem`, `TransactGetItems` | `TableName`, claves de `RequestItems`, o tablas de `TransactItems` |
| SSM | `PutParameter`, `DeleteParameter`, `DeleteParameters` | `GetParameter`, `GetParameters`, `GetParametersByPath` | `Name`, `Names`, `Path` |
| Secrets Manager | `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` | `GetSecretValue`, `BatchGetSecretValue` | `SecretId`, `SecretIdList`, `Name` (`CreateSecret`) |

En JavaScript cada operación tiene el sufijo `Command`; boto3 usa snake case
(`put_object`, `get_secret_value`). SNS, EventBridge y Kinesis son productores; S3, DynamoDB,
SSM y Secrets Manager usan los lados escritor y lector de recursos. Los nombres literales y ARN compatibles se unen
a la infraestructura por nombre físico, sin cuenta ni región. Los lados SDK literales se unen
en `exact`, incluso a otros lados SDK del mismo repositorio; un lado declarado en `meada.toml`
conserva `declared`. Los valores de infraestructura sustituibles siguen dando `inferred`.

Sin una entrada correspondiente en el entorno del workload, una lectura como `process.env.BUCKET` u `os.getenv("BUCKET")` queda sin resolver, con
`via: "env:BUCKET"` en el export y la variable nombrada en el contexto de su función. Un
productor o escritor SDK también tiene `via`, opcional en el formato 1; un literal usa `sdk:aws`.
Meada no lee el entorno del proceso. La construcción aislada de un comando, los publishes SNS
para teléfonos o endpoints, los clientes de alto nivel `boto3.resource` y `DynamoDBDocumentClient`
y las operaciones fuera de la tabla no se reconocen por estas formas.

`GetParametersByPath` consulta una jerarquía, por lo que su lado queda sin resolver; la ruta no se trata como nombre de parámetro. Un `ConditionCheck` de `TransactWriteItems` es lector, aunque la transacción también escriba otras tablas. Las claves calculadas de `RequestItems` conservan el literal o el nombre de la variable de entorno.

## Imágenes y código de las cargas de trabajo

El contrato `image::<registry>/<repository>` conecta una carga de trabajo con el repositorio
cuya configuración construye su imagen (#662). `nginx:1.27` pasa a ser
`image::docker.io/library/nginx`; `acme/api`, `image::docker.io/acme/api`. Los nombres quedan en
minúsculas, sin etiqueta ni digest.

Los constructores proceden de `docker/build-push-action` (`tags`), `docker/metadata-action`
(`images`), Compose con `build` e `image`, artefactos Skaffold (incluso varios documentos YAML),
destinos Jib literales de Maven/Gradle y comandos ko `build --bare` o `publish --bare` con
`KO_DOCKER_REPO` definido en el workflow. Las imágenes base no son destinos. Otros modos de
nombres de ko y expresiones Gradle no leídas no crean un constructor supuesto.

Los ejecutores son containers e init containers de cargas Kubernetes, después de aplicar los
overrides de imagen de Kustomize, servicios Compose e `image.repository` de valores Helm.
No se renderizan templates Helm. `context` muestra la imagen y los repositorios constructores;
`trace` recorre la relación en ambos sentidos hasta las entradas del código. Los overlays que
usan la misma carga e imagen conservan un ejecutor y su enlace por raíz de evaluación. Las
evidencias repetidas de esa imagen en una carga y raíz forman un solo lado. Un build
Compose local ejecuta directamente el código del mismo repositorio que su Dockerfile toma del
contexto (fuentes de `COPY`, `ADD` y `RUN --mount=type=bind`, en todas las etapas) en `exact`,
incluso sin `image`, dentro de la raíz y sin atravesar enlaces simbólicos, o en `inferred` cuando
el contexto o la ruta del Dockerfile pasó por un valor reemplazable. Un Dockerfile que no se puede
leer deja el contexto entero en `inferred`, y el motivo lo dice. `.dockerignore` no se lee.

Los literales enlazan en `exact`; los defaults Compose y valores Helm, en `inferred`, con su
origen en el motivo. Las partes no resueltas permanecen patrones y solo corresponden a nombres
literales en límites de segmentos; varios candidatos quedan `ambiguous` y admiten una decisión.
`status` solo enumera imágenes huérfanas cuando el ámbito visible tiene constructores y
ejecutores. En caso contrario, `search` y `context` marcan el lado sin contraparte. La
exportación mantiene el formato 1, con ubicaciones `at` y `executions` para rutas directas;
el grafo del console muestra contratos de imagen, enlaces y código de cargas de trabajo.

### Nombres parcialmente resueltos

Un canal, recurso o imagen con partes desconocidas conserva un patrón como `*-orders`.
`${terraform.workspace}-orders` coincide con `prod-orders` en `inferred`: se requieren
al menos tres letras o dígitos literales (los separadores no cuentan) y cada `*` debe ocupar
segmentos completos delimitados por `-`, `_`, `.` o `/`. Un patrón nunca coincide con otro patrón.
Varios nombres literales compatibles quedan `ambiguous`, todos enumerados, y se convierten en
elementos abiertos. `meada decide` guarda la elección en `meada.decisions.toml`; el vínculo elegido
queda `decided`. Las decisiones de aprovisionamiento pertenecen al lado que contiene el patrón y
conservan su raíz de evaluación.

Los elementos de claves de configuración usan metadatos del archivo, la línea, la clave y el contrato, también cuando el valor está marcado `redacted` o recortado.

Un patrón de builder de imagen con varios nombres literales de runners también mantiene su propio elemento abierto en el repositorio que construye la imagen.

## Ubicaciones de recursos declarados

El `via` de `[[resource]]` acepta símbolos (`src/orders.ts::cancelOrder`), direcciones IaC y
`<archivo>#<ruta de clave>`. El lado pertenece al bloque o la clave, accesible por `context` y
`trace`, y sus enlaces siguen siendo `declared`. Las direcciones incluyen Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) y Serverless
(`serverless.yml::functions.api`). Use `<archivo>::<dirección>` para distinguir archivos. La ubicación identifica el bloque de
origen; el calificador ` (root ...)` de consultas IaC no es una forma de `via`.
Una dirección que coincide con varios bloques de origen necesita el calificador de archivo;
sin él, el `via` está obsoleto.

Las rutas usan puntos, índices (`groups[0].name`) y claves entre comillas JSON
(`metadata.labels["app.kubernetes.io/name"]`). En listas, `[clave=valor]` selecciona elementos
por igualdad del valor completo, por ejemplo
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
La ubicación procede de valores indexados y sus rutas padre.
Los valores nulos y contenedores vacíos no tienen valor indexado; su `via` está obsoleto.
Un selector puede encontrar varios elementos; cada clave encontrada recibe un lado. Los
valores marcados como `redacted` o recortados no sirven para seleccionar elementos. Las expresiones
y plantillas HCL no satisfacen la igualdad de un selector. Los archivos siguen
las reglas del índice, incluido `.meadaignore`; un `via` no abre archivos por su cuenta.

Un `via` sin coincidencia está obsoleto y aparece en las líneas existentes de `meada status`.
Sin ningún `via` válido, el lado vuelve al repositorio completo. Esto conecta etiquetas en
valores Helm con reglas de enrutamiento mediante declaraciones sin renderizar plantillas.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```

## Funciones y tareas AWS

Lambda en Terraform/OpenTofu ejecuta `image_uri`; SAM ejecuta `ImageUri`, y las tareas ECS
ejecutan la imagen de cada contenedor de `container_definitions`, incluso mediante `jsonencode`.
Estos ejecutores participan en contratos de imagen, contexto, trazas y huérfanos. El entorno
conserva su contenedor y raíz de evaluación, sin alcanzar código de otra imagen.

Un `filename` local de Lambda o `CodeUri` de SAM conecta directamente el código del mismo
repositorio. Una referencia exacta a `data.archive_file.<nombre>.output_path` puede seguir
solo su entrada local explícita `source_dir` o `source_file`. Los archivos ZIP son ubicaciones
opacas; su contenido no se extrae. Un handler Serverless como `worker/send.dispatch` selecciona
un único archivo local existente (`.ts`, `.js`, `.mjs`, `.cjs` o `.py`); las extensiones ambiguas
quedan sin resolver. Las rutas nuevas deben ser relativas, sin componentes `..` y sin
atravesar enlaces simbólicos. S3, URL y código remoto no se convierten en rutas locales.

SAM combina `Globals.Function.Environment.Variables` con el entorno de la función; Serverless
combina `provider.environment` con `functions.<nombre>.environment`. La función prevalece,
aunque su valor de reemplazo no pueda leerse. Terraform Lambda y ECS también aportan sus
entornos. Las referencias conocidas a recursos pueden conservar la identidad del contrato
sin inventar un ARN o URL del despliegue. Secretos, referencias dinámicas y valores externos
siguen sin contenido. La lectura en código resuelve en `inferred`, con workload y variable
en el motivo; `context` y `trace` muestran la ejecución por imagen y las rutas directas.

Referencias como `aws_sqs_queue.orders.url` y `aws_s3_bucket.assets.bucket` identifican
el contrato sin proporcionar una URL del despliegue; atributos de contenido como `value` siguen sin resolver.

## Entornos de los workloads

Una lectura de entorno que nombra una cola, tópico o recurso, como `process.env.QUEUE_URL`,
se resuelve solo con las entradas del workload que ejecuta ese código. La evidencia de
ejecución viene de la imagen publicada por el repositorio o de un contexto de build local
de Compose. Los contenedores conservan sus propios entornos. La resolución es `inferred`,
con el workload y la variable en el motivo.

Kubernetes aporta literales, referencias a claves de ConfigMap o Secret y `envFrom`; Compose
aporta entradas en mapa o lista y valores predeterminados como `${QUEUE_URL:-orders}`.
`context` del workload muestra los nombres, destinos y valores disponibles. Un Secret sigue
`redacted`, y una referencia ausente o un valor no resuelto no crea vínculos. Los nombres solo
coinciden dentro del ámbito del workload: los valores vienen del IaC y la configuración de
los repositorios, nunca del entorno de un proceso.

## Alertas y rutas de Alertmanager

El contrato `alert::<clave>::<valor>` une las etiquetas de reglas Prometheus y Loki
(incluido `PrometheusRule`) y Grafana con los matchers de rutas de Alertmanager, en
archivos de configuración y objetos `AlertmanagerConfig`. La igualdad crea un enlace
`exact`; una expresión regular crea `inferred`. Las condiciones de una ruta se comprueban juntas.
Una condición negada (`!=`, `!~`) solo filtra la ruta: no crea lado ni enlace para el valor
que excluye.

Una expresión regular que el `regexp` de Go no compila (un grupo `(` sin cerrar, un `*` al
inicio, un rango invertido `[z-a]`, un lookahead `(?=`) hace que Alertmanager rechace la
configuración entera, así que Meada lee esa configuración, o ese objeto `AlertmanagerConfig`,
como ilegible: no crea lado de alerta ni enlace y `meada status` cuenta el archivo en
`ilegibles`. La misma configuración con la expresión corregida enlaza con normalidad. Una
expresión que Meada deja sin correspondencia solo por sus propios límites, o porque no lee la
sintaxis, vale para Alertmanager hasta donde Meada sabe: su matcher aparece como huérfano y
las demás rutas siguen enlazando.

`search` con kind `alert` encuentra el contrato; `context` en la ruta muestra su receiver,
y `trace` recorre de la regla a la ruta y de vuelta. `meada export` incluye emisores,
matchers y enlaces. `meada status` solo lista huérfanos cuando el ámbito visible contiene
ambos lados; de lo contrario, `search` y `context` indican el lado ausente. Los valores Helm
que dependen de templates no son emisores: Meada no renderiza templates.

Las reglas Grafana también se leen de exports JSON y bloques `rule` de recursos `grafana_rule_group` en Terraform/OpenTofu, con etiquetas literales.

Una ruta raíz sin matchers no crea un lado de alerta: no hay clave y valor para el contrato. El grafo describe los matchers escritos, sin predecir notificaciones finales, inhibiciones ni horarios.

Prometheus y Loki también emiten el `alertname` literal del nombre de la regla, sobrescribiendo una etiqueta con ese nombre. Las etiquetas que puedan venir de la expresión permanecen desconocidas, no vacías. Los archivos bajo `templates/` no se renderizan ni se tratan como emisores de alertas.

### OpsGenie y PagerDuty en Terraform

Las tags literales `clave:valor` de `datadog_monitor` son emisores. Las condiciones de
tags en `opsgenie_team_routing_rule` y las condiciones
PCL de orquestaciones PagerDuty, son matchers. La igualdad es `exact`; regex, prefijo y
contención son `inferred`. Las condiciones y el orden de las reglas se evalúan juntos;
valores desconocidos u operaciones no leídas no eligen un destino por conjetura.

Las tags de Datadog llegan a OpsGenie. Las etiquetas de reglas llegan a tags sólo mediante
un receiver OpsGenie seleccionado en Alertmanager que configure `tags` explícitamente.
Los templates simples de `.CommonLabels.<clave>` o `.GroupLabels.<clave>` usan etiquetas
fijadas por el `group_by` efectivo. Un receiver PagerDuty v2 puede reenviarlas por
`details`, que se convierte en `event.custom_details.<clave>`; no hay reenvío implícito
de tags a PagerDuty. Templates no leídos y reenvíos ausentes no generan links.

`context` muestra el ámbito escrito de la regla, y `trace` conserva el camino hasta el
bloque de enrutamiento. Un matcher compatible no demuestra entrega ni una notificación
final a un equipo. En el ejemplo público #638 la regla prueba `service:app`: el mismo
monitor también emite `support:team-b`, pero no hay allí un matcher para `support`.
El YAML de monitor no estándar y los valores Helm originales siguen siendo configuración.

## Contratos Azure y Functions

La tabla Terraform/OpenTofu lee `azurerm_servicebus_queue`, `azurerm_servicebus_topic` y
`azurerm_servicebus_subscription` como canales `servicebus`; `azurerm_eventhub` como
`eventhubs`; `azurerm_storage_account` como recurso `azurestorage`,
`azurerm_storage_container` como `azureblob` y `azurerm_key_vault_secret` como `keyvault`.
Una cuenta y un contenedor con igual nombre mantienen contratos distintos.

Una suscripción tiene identidad `channel::servicebus::subscription:<tema>/<suscripción>`.
Une sus consumidores con los productores del tema, con `exact` sin regla de filtro e
`inferred` cuando se declara una regla. Las colas y temas conservan `/` en sus nombres.
Los processors C# existentes conservan la identidad del tema, incluso al recibir una
suscripción. Los IDs ARM completos y las URL de blob/secreto admitidas dan usos `exact`;
las claves finales `servicebus_queue`, `servicebus_topic`, `servicebus_subscription`
(valor `tema/suscripción`), `eventhubs_hub`, `azurestorage_account`, `azureblob_container`
y `keyvault_secret` dan `inferred`. La igualdad sin esta evidencia no crea vínculos.

`azurerm_linux_function_app` lee el bloque Docker de `site_config.application_stack`;
ambos tipos Linux y Windows leen `zip_deploy_file` local y `app_settings`. El archivo local
queda dentro del repositorio, sin `..` ni symlinks; los ZIP no se abren. Un paquete remoto
no identifica código local. Las referencias Key Vault en app settings siguen ocultas
y sin resolver. Los contratos y workloads participan en `context`, `trace`, huérfanos,
aprovisionamiento y export.

### SDK de Azure en cinco grupos de lenguajes

Los constructores conocidos de los SDK de Azure en JavaScript/TypeScript, Python, Java, Go y C# crean productores/consumidores de Service Bus y Event Hubs, escritores/lectores de contenedores Blob y lectores de secretos Key Vault. La [guía de usuario](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.es.md#uso-de-los-sdk-de-azure-en-cinco-grupos-de-lenguajes) enumera constructores y operaciones. Los receptores de suscripción conservan tema y suscripción dentro de la misma carga, raíz y contenedor; los nombres del entorno sin esa evidencia siguen sin resolverse. Las formas C# existentes de Service Bus conservan la identidad del tema.

## Recursos personalizados y módulos de infraestructura

Este es un inventario cerrado de 43 tipos de recursos personalizados, más los roles de Exchange y Binding del operador RabbitMQ. Los tipos y grupos deben coincidir exactamente. Otros CRD siguen disponibles en `search` con `kind = "iac"`, `context` y `trace`, sin recibir un contrato supuesto. Meada lee los archivos del repositorio sin consultar operadores ni descargar módulos.

| Tipo y grupo | Campo de identidad | Contrato |
| --- | --- | --- |
| `KafkaTopic.kafka.strimzi.io` | `spec.topicName` | `channel::kafka` |
| `Queue.rabbitmq.com` | `spec.name` | `channel::amqp` |
| `Queue.sqs.services.k8s.aws` | `spec.queueName` | `channel::sqs` |
| `Bucket.s3.services.k8s.aws` | `spec.name` | `resource::s3` |
| `Topic.sns.services.k8s.aws` | `spec.name` | `channel::sns` |
| `EventBus.eventbridge.services.k8s.aws` | `spec.name` | `channel::eventbridge` |
| `Stream.kinesis.services.k8s.aws` | `spec.name` | `channel::kinesis` |
| `Table.dynamodb.services.k8s.aws` | `spec.tableName` | `resource::dynamodb` |
| `DBInstance.rds.services.k8s.aws` | `spec.dbInstanceIdentifier` | `resource::rds` |
| `DBCluster.rds.services.k8s.aws` | `spec.dbClusterIdentifier` | `resource::rds` |
| `CacheCluster.elasticache.services.k8s.aws` | `spec.cacheClusterID` | `resource::elasticache` |
| `ReplicationGroup.elasticache.services.k8s.aws` | `spec.replicationGroupID` | `resource::elasticache` |
| `Parameter.ssm.services.k8s.aws` | `spec.name` | `resource::ssm` |
| `Secret.secretsmanager.services.k8s.aws` | `spec.name` | `resource::secretsmanager` |
| `PubSubTopic.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `PubSubSubscription.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `StorageBucket.storage.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcs` |
| `SQLInstance.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SQLDatabase.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SecretManagerSecret.secretmanager.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcpsecretmanager` |
| `Queue.sqs.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sqs` |
| `Bucket.s3.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::s3` |
| `Topic.sns.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sns` |
| `Bus.cloudwatchevents.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventbridge` |
| `Stream.kinesis.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::kinesis` |
| `Table.dynamodb.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::dynamodb` |
| `Instance.rds.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::rds` |
| `Cluster.elasticache.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::elasticache` |
| `Parameter.ssm.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::ssm` |
| `Secret.secretsmanager.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::secretsmanager` |
| `Topic.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Subscription.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Bucket.storage.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcs` |
| `DatabaseInstance.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Database.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Secret.secretmanager.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcpsecretmanager` |
| `Queue.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Topic.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Subscription.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `EventHub.eventhub.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventhubs` |
| `Account.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azurestorage` |
| `Container.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azureblob` |
| `Secret.keyvault.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::keyvault` |

Strimzi usa `metadata.name` solo cuando falta `spec.topicName`. Config Connector usa `metadata.name` solo cuando falta `spec.resourceID`. Un campo presente pero desconocido o inválido no activa ese valor por defecto. Las familias Crossplane enumeradas requieren la anotación explícita `crossplane.io/external-name`. El nombre físico mostrado en el nodo sigue separado de su identificador de contrato normalizado.

Cloud SQL usa la identidad compuesta `<instancia>/<base>`: `SQLDatabase` lee `spec.instanceRef`, y `Database.sql.gcp.upbound.io` lee `spec.forProvider.instance` o `instanceRef`. Las referencias locales deben resolver a un solo objeto del grupo, tipo y namespace correctos. Las suscripciones Pub/Sub tienen contrato propio `channel::pubsub::subscriptions/<nombre>` y leen el origen de `spec.topicRef` o `spec.forProvider.topic`/`topicRef`. Las suscripciones Service Bus requieren el nombre completo o ARM ID que identifica tópico y suscripción, produciendo `channel::servicebus::subscription:<tópico>/<suscripción>`. Un nombre corto sin calificación no basta.

Las bases literales de Composition usan el mismo inventario. Los patches que pueden cambiar identidad, calificación o ruta suprimen la proyección; patches desconocidos, conjuntos ausentes o duplicados y conjuntos anidados tampoco establecen un contrato. Las funciones y los patches nunca se ejecutan.

### Topología RabbitMQ

`Queue.rabbitmq.com` provisiona `channel::amqp::<spec.name>`. `Exchange.rabbitmq.com` aporta nombre y tipo sin convertirse en canal. `Binding.rabbitmq.com` referencia `spec.source` y `spec.destination` en la misma unidad de evaluación, cluster, namespace y vhost; una referencia por `connectionSecret` sigue separada de una referencia por nombre de cluster. Solo destinos de tipo `queue` con endpoints únicos y conocidos generan binding. `topic` usa `*` y `#`; `direct` acepta claves sin comodines; `fanout` alcanza todas las claves. `headers`, tipos desconocidos y destinos de tipo `exchange` no generan ruta de cola. Estos últimos conservan referencias navegables. El tipo ausente de Exchange es `direct`. Los nombres y claves AMQP tienen un límite de 255 bytes. El provisionamiento de un publish conserva su lugar real y nivel de ruta, `exact` para clave literal e `inferred` para comodín.

### Módulos de registro

Solo se reconocen estas seis fuentes; se acepta el prefijo opcional `registry.terraform.io/`. La tabla enumera entradas leídas y controles de creación.

| Fuente | Entradas; controles |
| --- | --- |
| `terraform-aws-modules/sqs/aws` | `name`, `fifo_queue`, `use_name_prefix`; `create` |
| `terraform-aws-modules/sns/aws` | `name`, `fifo_topic`, `use_name_prefix`; `create` |
| `terraform-aws-modules/s3-bucket/aws` | `bucket`; `create`, `create_bucket`, `putin_khuylo` |
| `terraform-aws-modules/dynamodb-table/aws` | `name`; `create`, `create_table` |
| `terraform-aws-modules/lambda/aws` | `function_name`, `package_type`, `image_uri`, `environment_variables`, `source_path`, `local_existing_package`, `handler`; `create`, `create_function`, `create_package`, `create_layer`, `putin_khuylo` |
| `terraform-aws-modules/ecs/aws//modules/service` | `name`, `container_definitions` (`name`, `create`, `image`, `environment`, `secrets`); `create`, `create_service`, `create_task_definition` |

`create = false`, `count = 0` o un control de creación desconocido no produce lado. Los nombres desconocidos siguen como patrones seguros, sin inventar un nombre físico. SQS y SNS añaden `.fifo` cuando se solicita; los nombres generados por `use_name_prefix` no establecen provisionador literal. Lambda y ECS aportan imágenes y ambientes con el mismo lector de los workloads nativos, conservando ámbitos por contenedor. Lambda acepta `source_path` escalar conocido con `create_package = true`, dentro del repositorio y sin `..` ni symlink; las formas de lista/objeto y el empaquetado S3 no tienen destino local. `local_existing_package` debe ser un archivo existente y queda opaco: no se extraen ZIP ni se ejecuta empaquetador. `create_layer = true` suprime la función. El directorio del módulo no se supone como código de la función.
