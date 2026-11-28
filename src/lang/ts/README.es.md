# Referencias, receptores y usos en TypeScript y JavaScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

El adaptador registra el origen de los receptores, las referencias fuera de llamadas y los usos de
resultados que revelan la estructura de los datos.

## Interfaz

`References` reúne nombres usados como tipos, valores y elementos JSX. `inferred` reconoce
`z.infer<typeof S>`. `Uses::of` sigue lo que hace la función con el resultado de cada llamada.
[references.rs:References](./references.rs) [types.rs:inferred](./types.rs) [uses.rs:of](./uses.rs)

El adaptador registra la clase del receptor de una llamada a partir de parámetros tipados,
variables locales, campos de instancia y tipos de retorno declarados. `Promise<T>` aporta su tipo
interno solo tras `await`, cuando `Promise` designa el tipo global.
[receivers.rs:local](./receivers.rs) [receivers.rs:definition](./receivers.rs) [receivers.rs:result](./receivers.rs)

## Invariantes

Las llamadas de un archivo comparten un presupuesto de usos. Una vez agotado, los usos siguientes
quedan vacíos. Un tipo genérico oculto por una clase, interfaz o alias queda desconocido.
[uses.rs:USES_BUDGET](./uses.rs) [types.rs:reference](./types.rs)

Los constructores y espacios de nombres globales, como `Map` y `JSON`, solo cuentan donde ninguna
declaración o asignación oculta el nombre. La búsqueda de un parámetro de tipo que oculte una
referencia recorre hasta 64 ancestros.
[globals.rs:constructor](./globals.rs) [globals.rs:namespace](./globals.rs) [globals.rs:Bindings](./globals.rs) [types.rs:HIDING_CLIMB](./types.rs)

## Efectos secundarios

La recopilación elimina referencias duplicadas y mantiene una caché de usos por ámbito mientras el
árbol sintáctico está disponible.
[references.rs:push](./references.rs) [uses.rs:Uses](./uses.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- lang::ts uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
