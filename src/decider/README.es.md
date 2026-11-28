# Decisiones asistidas por modelo

Decisiones asistidas por modelo con la opción explícita `meada decide --decider`. Los adaptadores implementan los protocolos `chat` y `decisions`.

## Interfaz

La ejecución selecciona elementos elegibles, construye los fragmentos de contexto y entrega las decisiones aceptadas a la función suministrada. El protocolo de chat envía una pregunta por solicitud y el de decisiones las agrupa. `[mod.rs:run]` `[chat.rs:request]` `[decisions.rs:request]`

El modo automático informa y omite los elementos sin candidatos o con más de 50. `[mod.rs:MAX_CANDIDATES]` `[mod.rs:run]`

## Invariantes

Cada respuesta debe indicar una clave ofrecida, una probabilidad entre cero y uno y un identificador de modelo válido. `[validate.rs:proposal]`

## Efectos secundarios

Lee configuración del entorno y fragmentos del repositorio. La ejecución puede enviar esos fragmentos por HTTP y escribir el resultado en la salida suministrada. La vista previa de la solicitud termina antes del envío. `[mod.rs:from_env]` `[mod.rs:run]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it decider::`.
