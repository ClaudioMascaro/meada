# Selección de archivos y valores Go

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Determina qué archivos y paquetes entran en el análisis de Go y qué valores aporta una expresión
al grafo.

## Interfaz

`builds` y `listed` filtran archivos y directorios. `Modules` resuelve importaciones mediante
`go.mod`, incluido `replace`. `value` extrae literales, llamadas y partes de cadenas.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs) [modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

`env_fields` registra la variable de entorno indicada por la etiqueta `env` o `envconfig` de un
campo, incluida la ruta de campos en structs anónimas anidadas. `fmt.Sprintf` acepta una constante
de texto del archivo como formato cuando ese nombre sigue visible.
[values.rs:env_fields](./values.rs) [values.rs:env_tag](./values.rs) [values.rs:format](./values.rs)

## Invariantes

El destino siempre es `linux/amd64` con `gc`. Una restricción ilegible excluye el archivo. `./...`
omite `testdata` y directorios cuyo nombre empieza por `.` o `_`.
[build.rs:builds](./build.rs) [build.rs:listed](./build.rs)

El lector descarta una restricción de compilación que alcanza 64 niveles. Una llamada diferida que
pueda modificar el único resultado con nombre deja su valor de retorno desconocido.
[build.rs:CONSTRAINT_DEPTH](./build.rs) [../go.rs:deferred_write](../go.rs)

## Efectos secundarios

Los lectores construyen la tabla de módulos a partir de los bytes de configuración recibidos y
evalúan expresiones desde el árbol sintáctico.
[modfile.rs:Modules](./modfile.rs) [values.rs:value](./values.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::go gopkg:: uses:: base_url:: kafka_env::
```
