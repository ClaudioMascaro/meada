# Colas y temas en el código

Extracción de productores, consumidores, publicaciones y vínculos de colas o temas a partir del código de aplicación.

## Interfaz

Los lectores de C#, Go, Java, Python, Rust y JS/TS convierten llamadas de bibliotecas en mensajes. Las operaciones AWS comparten una tabla y los nombres de destino pasan por lectores comunes de argumentos. `[csharp.rs:messages]` `[go.rs:messages]` `[java.rs:messages]` `[python.rs:messages]` `[rust.rs:messages]` `[ts.rs:messages]` `[aws.rs:OPERATIONS]` `[message.rs:Message]`

En Go y JS/TS, `[[channel_client]]` identifica una función o método del repositorio e indica qué argumento o campo contiene el tema de Kafka. En Go también puede identificar una función por la ruta de importación. Una declaración que no produce ningún lado aparece en `status`. `[go.rs:declared]` `[go.rs:client_sides]` `[ts.rs:declared_sides]` `[../channel.rs:status]`

## Invariantes

Cada lenguaje exige evidencia de la biblioteca o el receptor previsto. Los lectores de publicaciones y vínculos AMQP mantienen separados exchange, clave de enrutamiento y cola. `[python.rs:messages]` `[ts.rs:messages]` `[message.rs:publish]` `[message.rs:bind]`

El lector de KafkaJS reconoce clientes en campos privados y consulta los valores literales de `app-config` cuando se lee directamente la clave del tema. Con varios valores o una configuración incompleta, la certeza máxima es `inferred`. La búsqueda de un receptor en C# se detiene al alcanzar 32 pasos. `[ts.rs:messages]` `[ts.rs:configured_topic]` `[csharp.rs:CHANNEL_CLIENT_DEPTH]`

## Efectos secundarios

Los lectores generan mensajes en memoria a partir de los hechos extraídos sin abrir conexiones con brokers. `[message.rs:Read]` `[ts.rs:messages]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it -- channels:: channel_clients:: kafkajs_classes:: kafka_env::`.
