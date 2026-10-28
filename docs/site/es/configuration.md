# Configuración

Cada repositorio puede tener un `meada.toml` en la raíz, versionado en git, que dice con quién
conversa. Quien consume una ruta nombra al par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sin esquema ni .git
base_url_env = ["API_URL"]               # variable de la que el cliente lee la URL base
# path = "../api"                        # opcional: pista de dónde está el clon
```

Quien sirve la ruta declara los hosts en los que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: lo que el gateway pone delante de las rutas
strip = "/v1"                            # opcional: lo que les quita del comienzo
```

Con `prefix` y `strip`, una llamada a `/shop/orders` llega a la ruta `/v1/orders` del código, en
el nivel `inferred` y con el motivo `gateway prefix /shop, gateway strip /v1`. Un segmento de
versión solo se quita cuando lo escribes en `strip`, y quien llama a la ruta del código sigue
enlazando como antes.

Un recurso compartido que ningún código nombra, como una tabla o un bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador en minúsculas
id = "orders"
role = "write"                           # read, write o read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: los símbolos que hacen el acceso
```

Cada escritor enlaza con cada lector del mismo recurso en otro repositorio, en el nivel
`declared`. Sin `via`, o si ningún `via` corresponde a un símbolo, el lado es el repositorio
entero.

Meada encuentra los pares por su remoto git, aunque el clon esté en otra ruta en cada máquina.
Una clave desconocida o mal escrita da un error con el archivo, la línea y la clave. Sin
`meada.toml`, el repositorio no declara ningún par.

## Artefactos JVM

Los contratos de paquetes JVM se descubren en los manifests: `pom.xml`, `settings.gradle`, `settings.gradle.kts`, `build.gradle` y `build.gradle.kts`. No necesitan una clave adicional en `meada.toml`. Declare otros repositorios en `[[peer]]` como en los ejemplos anteriores. Use coordenadas literales para vínculos `exact`; las propiedades Maven y expresiones Gradle quedan sin evaluar, y las que forman el grupo o el artefacto producen candidatos `ambiguous`.

En Java y Kotlin con Spring, la URL base puede venir de `@Value("${clave}")` (en Kotlin, `@Value("\${clave}")`, con el `$` escapado), leída del `application.properties`, del `application.yml` o del `application.yaml` en `src/main/resources` de cada módulo, sin clave nueva en `meada.toml`. Una URL literal vincula por host en `[serve] hosts`; un placeholder `${VAR}` (con o sin `:valor`) usa las mismas claves `base_url_env` de `[[peer]]`. Un valor por defecto cuyo host sirve algún par vincula por ese host.

En C#, la URL base puede venir de `Environment.GetEnvironmentVariable("X")` o de `configuration["Seccion:Clave"]`, leída del `appsettings.json` del proyecto (solo el archivo base), sin clave nueva en `meada.toml`. Una clave es la variable `Seccion__Clave` en los `base_url_env` del `[[peer]]`; una URL literal que el archivo guarda para ella vincula por host en `[serve] hosts` cuando algún peer sirve ese host.

## Paquetes NuGet

Los contratos de paquetes NuGet se descubren en los `.csproj` con SDK. No necesitan ninguna clave extra en `meada.toml`: un `PackageReference` enlaza con el proveedor por su id, y un `ProjectReference` por la ruta de su archivo de proyecto, que alcanza un proyecto de otro repositorio por las raíces de los repositorios del workspace (decláralo en `[[peer]]`). Valores de propiedad como `$(Version)` quedan sin evaluar.

## Paquetes Swift

Los contratos de paquete Swift se descubren en `Package.swift`. No necesitan una clave nueva en `meada.toml`: un `.package(url:)` enlaza con el repositorio del workspace de la misma identidad de remoto y un `.package(path:)` con el repositorio en esa ruta, que debe ser un `[[peer]]` del workspace cuando es otro repositorio. Use URLs, rutas y nombres literales para enlaces `exact`.


## Qué se indexa

El recorrido cubre todos los archivos del repositorio, versionados o no, salvo lo que estos excluyen:

- **Siempre omitidos**, los liste o no un archivo de ignore: directorios llamados `node_modules` o `Pods`, un directorio `Checkouts` dentro de `Carthage`, archivos llamados `*.min.js` o `*.min.css` y la salida de build de .NET (`obj` o `bin` junto a un `.csproj`). Son cachés de dependencias y artefactos de build, nunca código propio.
- **`.gitignore` y `.ignore`**, respetados con o sin git, en la raíz y en subdirectorios, y solo dentro del repositorio.
- **`.meadaignore`**, con la sintaxis de `.gitignore`, leído de la misma forma. No devuelve un archivo que los valores por defecto omiten.

`vendor`, `dist`, `build` y el código generado (las migraciones de Entity Framework, por ejemplo) no se omiten por defecto: cualquiera puede ser código propio o contener un contrato, y Meada no puede distinguirlo. Lista en `.meadaignore` lo que no quieras indexar:

```gitignore
vendor/
dist/
build/
Migrations/
*ModelSnapshot.cs
*.Designer.cs
```

Un archivo que un nuevo valor por defecto o una nueva entrada excluye sale del índice en el siguiente refresh, como un archivo borrado.
