# Campos y métodos de objetos en TypeScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

El resolvedor identifica los valores de campos y métodos de objetos literales que siguen siendo
válidos en el punto de lectura.

## Interfaz

`ts_construction` resuelve la clase donde se escribió `new`. `ts_literal_method` busca el método
de un objeto literal. `ts_field_argument` recupera el argumento que recibe un campo en una
construcción exacta, incluso a través de una fábrica. `ts_record` selecciona la propiedad para una
clave literal o reúne los valores posibles cuando la clave es dinámica.
[fields.rs:ts_construction](./fields.rs) [fields.rs:ts_literal_method](./fields.rs) [fields.rs:ts_field_argument](./fields.rs) [fields.rs:ts_record](./fields.rs)

## Invariantes

Una escritura distinta de la inicialización o un índice de escrituras incompleto puede invalidar
un campo. Las lecturas de producción usan las escrituras de producción. Las lecturas en pruebas
consideran también las escrituras de pruebas. Una subclase conocida o un parámetro reasignado deja
desconocido el argumento del constructor durante la propagación.
[fields.rs:ts_field_written](./fields.rs) [fields.rs:ts_field_argument](./fields.rs)

## Efectos secundarios

`Writes::index` recorre los hechos del repositorio y crea índices en memoria por propietario y
nombre del campo. Conserva la ubicación de una escritura única para distinguir la inicialización
de las demás escrituras.
[fields.rs:Writes](./fields.rs) [fields.rs:index](./fields.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- ts_receivers:: field_flow:: http_bases::
```
