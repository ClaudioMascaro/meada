# Resolución de llamadas y valores

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

El resolvedor combina el ámbito, la clase del receptor y las escrituras para encontrar los
destinos de llamadas y determinar el valor disponible en cada lectura.

## Interfaz

`value_within_at` evalúa el valor en el punto de lectura. `member_changed` consulta escrituras del
código y mutaciones adicionales reconocidas por el lector de la biblioteca.
[mutations.rs:value_within_at](./mutations.rs) [mutations.rs:member_changed](./mutations.rs)

Para TypeScript, el resolvedor selecciona la declaración visible en el punto de llamada,
identifica la clase del receptor y comprueba las redefiniciones en las subclases. Una llamada a un
método de un valor global conocido queda sin candidato del repositorio. Una anotación de
biblioteca aún puede admitir métodos de objetos literales, dobles de prueba y clases que declaran
ese tipo en su jerarquía.
[ts.rs:ts_local](./ts.rs) [ts.rs:receiver_class](./ts.rs) [ts.rs:ts_override](./ts.rs) [ts.rs:ts_library_call](./ts.rs)

- [Campos y métodos de objetos en TypeScript](./ts/README.es.md)

## Invariantes

Una escritura posterior a la lectura puede afectar la siguiente iteración del mismo bucle. Un
índice incompleto o un presupuesto agotado impide usar el campo como valor conocido.
[mutations.rs:looped](./mutations.rs) [mutations.rs:changed](./mutations.rs)

Al evaluar un retorno de Python, un decorador fuera de `PY_KEEPS_BODY` en la función, en su clase
o en una subclase que pueda sustituir el método deja el resultado desconocido. En la JVM, una
llamada exacta a un método privado conserva ese cuerpo aunque otra clase declare el mismo nombre.
[../resolve.rs:py_decorated](../resolve.rs) [../resolve.rs:PY_KEEPS_BODY](../resolve.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Efectos secundarios

La primera consulta crea un índice en memoria para el lector de mutaciones. Las lecturas
siguientes reutilizan el índice y descuentan el trabajo por archivo.
[mutations.rs:member_changed](./mutations.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- resolve:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases:: base_url:: xlang::
```
