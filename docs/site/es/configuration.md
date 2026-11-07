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
via = ["src/orders.ts::cancelOrder"]     # opcional: las ubicaciones que hacen el acceso
```

Cada escritor enlaza con cada lector del mismo recurso en otro repositorio, en el nivel
`declared`. Sin `via`, o si ningún `via` corresponde a un símbolo, nodo IaC o clave, el lado es el repositorio
entero.

Los SDK AWS de Java 2.x, Go v2 y .NET también nombran recursos directamente desde el código: por ejemplo, `PutObject` en `uploads` escribe `resource::s3::uploads`, y `GetObject` en ese bucket lee el mismo contrato. Los usos escritos enlazan en `exact`; una declaración explícita conserva `declared`. Un bloque Terraform `aws_s3_bucket` del mismo nombre enlaza a todos estos usos como aprovisionador. No hace falta repetir los usos como `[[resource]]`. Construya el cliente en código o páselo mediante llamadas exactas; el tipo de una interfaz inyectada por sí solo no basta. Los nombres del entorno quedan sin resolver, con la variable en el export. Consulte las [operaciones reconocidas](capabilities.md#sdk-aws-en-java-go-y-c).

Una ruta que solo registra un archivo de prueba (`*_test.go`, `*.test.*`, `*.spec.*`,
`test_*.py`, `*_test.py`, un directorio `tests` o `__tests__`, `src/test` en JVM) no es
proveedora del repositorio: un enrutador o un upstream falso de prueba no sirve nada, y contarlo
haría que el repositorio cliente pareciera proveedor de la ruta que consume. El código de prueba
sigue indexado, con sus llamadas y símbolos. Para conservar esas rutas como proveedoras, activa
la clave en la raíz de `meada.toml`:

```toml
test_routes = true                       # opcional: las rutas registradas en archivos de prueba sirven
```

La clave solo cuenta en el `meada.toml` del propio repositorio (una entrada del manifiesto de workspace no la tiene) y solo para rutas HTTP; una ruta es de prueba por el archivo donde se define su handler.

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


## Mapeos privados de IaC

Asigne una fuente de módulo o un grupo/tipo personalizado a una entrada existente de la tabla de tipos. `entry` usa `<formato>:<tipo>`, por ejemplo `terraform:aws_sqs_queue`. El ejemplo lee el nombre del argumento declarado por el módulo o del campo del objeto:

```toml
[[iac.module]]
source_prefix = "git::https://example.com/platform/modules"
entry = "terraform:aws_sqs_queue"
input = "queue_name"

[[iac.module]]
source = "git::https://example.com/platform/modules/priority"
entry = "terraform:aws_sns_topic"
input = "settings.topic"

[[iac.custom_resource]]
group = "platform.example.com"
kind = "MessageQueue"
entry = "terraform:aws_sqs_queue"
key_path = "spec.endpoints[0].name"
```

Cada `[[iac.module]]` elige exactamente uno entre `source` y `source_prefix`. Gana una fuente exacta; después gana el prefijo más largo, limitado a una frontera `/` o `?`, sin capturar un vecino como `modules-other`. `input` admite puntos, índices y claves entre comillas JSON, sin selectores. `[[iac.custom_resource]]` requiere `group`, `kind`, `entry` y `key_path`, con la misma sintaxis. Los mapeos de módulo también pueden seleccionar `terraform:aws_lambda_function` o `terraform:aws_ecs_task_definition`, con las entradas Lambda/ECS documentadas en capacidades; otros workloads se rechazan. Los recursos personalizados seleccionan entradas de provisionamiento. El valor debe aportar la identidad requerida por la entrada, incluidos calificadores. Una entrada desconocida causa error que cita su nombre y `meada.toml`. Hay como máximo 256 mapeos, con textos y rutas de hasta 1 KiB. La configuración participa en el índice: cambiar o quitar el mapeo recalcula los nodos afectados. Ningún módulo se descarga ni se ejecuta.
