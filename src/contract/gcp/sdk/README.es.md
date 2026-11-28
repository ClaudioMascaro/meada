# Llamadas a los SDK de Google Cloud por lenguaje

Formas de los SDK de Google Cloud en C#, Go, Java, Python y JS/TS para Pub/Sub, Cloud Storage y Secret Manager.

## Interfaz

Los lectores identifican el cliente y devuelven el rol, el tipo de destino y el argumento que lo nombra. En Go, los temas y suscripciones se identifican a partir del cliente de Pub/Sub. `[csharp.rs:read]` `[go.rs:step]` `[java.rs:read]` `[python.rs:read]` `[ts.rs:read]`

## Invariantes

En Node, el método `on` de una suscripción solo representa consumo cuando recibe el evento literal `message`. Cada lenguaje comprueba la etapa del receptor antes de asignar dirección. `[ts.rs:read]` `[go.rs:read]`

## Efectos secundarios

Los lectores consultan hechos del código en memoria. `[java.rs:constructed]` `[python.rs:read]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it gcp_sdk::`.
