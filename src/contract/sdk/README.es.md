# Origen de clientes y argumentos de SDK

Evidencia compartida de construcción de clientes de SDK y lectura de los argumentos de una solicitud.

## Interfaz

La política de cada servicio indica cómo reconocer el cliente. Los lectores interpretan inicializadores de C#, literales compuestos de Go, builders de Java y mutaciones de mapas. `[origins.rs:Policy]` `[csharp.rs:field_of]` `[go.rs:field_of]` `[java.rs:Builders]` `[mutations.rs:writes]`

## Invariantes

Todas las alternativas deben producir la misma evidencia. Una alternativa opaca impide confirmar el origen. El presupuesto limita nuevas expansiones y el lector puede reutilizar evidencia ya calculada. `[origins.rs:Evidence]`

## Efectos secundarios

La evidencia y los resultados del análisis de builders se conservan en memoria durante la lectura. `[origins.rs:Evidence]` `[java.rs:Builders]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it aws_sdk::`.
