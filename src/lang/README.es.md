# Adaptadores de lenguaje

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Cada adaptador transforma el código fuente en hechos compartidos por la resolución y los
extractores de contratos.

## Interfaz

`LanguagePack` define el análisis por archivo, la configuración, la unión de hechos y la
resolución de módulos. `pack_for` selecciona por ruta. El registro incluye JavaScript/TypeScript,
Python, Rust, Go, Java, Kotlin, C# y Swift.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:pack_for](./mod.rs) [mod.rs:PACKS](./mod.rs)

El adaptador de C# reúne los `global using` del proyecto antes de resolver nombres. Java y Kotlin
leen la estructura de la JVM. Los demás adaptadores mantienen su propia lógica de descubrimiento
de módulos.
[csharp.rs:join](./csharp.rs) [java.rs:layout](./java.rs) [kotlin.rs:resolves_with](./kotlin.rs)

Go registra usos de tipos con nombre en firmas y declaraciones. Omite los tipos predefinidos y los
parámetros de tipo. El resolvedor descarta referencias Go sin destino en el repositorio.
[go.rs:type_uses](./go.rs) [go.rs:PREDECLARED](./go.rs) [go.rs:drops_external_uses](./go.rs) [../resolve.rs:reference](../resolve.rs)

`Facts` conserva por archivo los receptores y sus clases (`TsLocal`, `TsClass`), los campos Go
vinculados al entorno (`EnvField`) y las asignaciones observadas en campos privados
(`FieldWrites`). La resolución entre archivos y la extracción de contratos usan estos hechos.
[mod.rs:Facts](./mod.rs) [mod.rs:TsLocal](./mod.rs) [mod.rs:TsClass](./mod.rs) [mod.rs:EnvField](./mod.rs) [mod.rs:FieldWrites](./mod.rs)

- [Selección de archivos y valores Go](./go/README.es.md)
- [Metadatos de proyectos Python](./python/README.es.md)
- [Módulos, macros y valores de Rust](./rust/README.es.md)
- [Módulos y valores Swift](./swift/README.es.md)
- [Referencias, receptores y usos en TypeScript y JavaScript](./ts/README.es.md)

## Invariantes

Un paquete recibe los bytes y la ruta del archivo relativa al repositorio. La configuración y los
hechos de otros archivos proceden del indexador. Kotlin comparte el grupo de resolución de Java.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:resolver_of](./mod.rs) [kotlin.rs:resolves_with](./kotlin.rs)

El adaptador C# sigue hasta 128 tipos envolventes al extraer definiciones anidadas. Java y Kotlin
marcan métodos privados para que el resolvedor reconozca las llamadas que solo pueden ejecutar ese
cuerpo.
[csharp.rs:DEFINITION_DEPTH](./csharp.rs) [java.rs:parse](./java.rs) [kotlin.rs:parse](./kotlin.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Efectos secundarios

El análisis produce `Facts`. `hash_spans` completa hashes a partir de los bytes recibidos.
`with_values` habilita temporalmente la captura de valores en el hilo y restaura el estado al
salir.
[mod.rs:Facts](./mod.rs) [mod.rs:hash_spans](./mod.rs) [mod.rs:with_values](./mod.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang:: xlang:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
