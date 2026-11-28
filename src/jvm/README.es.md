# Builds JVM y configuración de Spring

Lectores JVM compartidos por los packs de Java y Kotlin y por los tipos de contrato que usan Maven, Gradle o Spring.

## Interfaz

Maven y Gradle proporcionan coordenadas, dependencias y metadatos de build. El lector de Spring resuelve propiedades y devuelve el valor de configuración de una anotación `@Value` reconocida. `[maven.rs:read]` `[gradle.rs:read]` `[mod.rs:Build]` `[spring.rs:resolve]` `[spring.rs:inject]`

## Invariantes

Las coordenadas literales validan el grupo y el artefacto. La configuración de Spring considera los archivos base del módulo y da prioridad a `application.properties` sobre YAML. `[mod.rs:coordinate]` `[spring.rs:Configs]`

El lector de Gradle devuelve un resumen vacío cuando los bloques superan los 64 niveles de anidamiento. `[gradle.rs:GRADLE_DEPTH]`

## Efectos secundarios

Los lectores interpretan el contenido suministrado sin ejecutar Maven, Gradle ni la aplicación Spring. `[maven.rs:read]` `[gradle.rs:read]` `[spring.rs:Configs]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it jvmpkg::`.
