# Llamadas a los SDK de Azure por lenguaje

Formas de llamada de los SDK de Azure para C#, Go, Java, Python y JS/TS.

## Interfaz

Cada lector reconoce constructores importados, sigue clientes derivados y clasifica la operación final. El lector de entorno combina las partes de la identidad del receptor. `[csharp.rs:constructed]` `[go.rs:step]` `[java.rs:operation]` `[python.rs:read]` `[ts.rs:read]` `[environment.rs:environment_identity]`

## Invariantes

Cuando el tema y la suscripción de Service Bus proceden de variables de entorno, sus nombres deben ser inequívocos en el mismo ámbito de ejecución. Los literales pueden usarse directamente. `[environment.rs:environment_identity]`

Al combinar valores de un mismo ámbito, el texto debe ser no vacío y todas las coincidencias deben indicar de igual modo si el valor es una referencia tipada. `[environment.rs:unique]`

## Efectos secundarios

El análisis trabaja en memoria sobre llamadas y valores ya recopilados. `[ts.rs:read]` `[environment.rs:environment_identity]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it azure_sdk::`.
