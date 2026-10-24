# Generaciones y retención

**Puntero de generación y pins** (feature `generation-pointer`, que incluye `snapshot`;
`cargo install meada --features generation-pointer`). Con la sección `[generations]`, que exige
`[snapshot]` y `[valkey]`, el puntero de la generación actual pasa del store a Valkey, y toda la
flota guarda el mismo conjunto de generaciones.


<!-- meada-graph:generation-lifecycle -->

```toml
[valkey]
url = "rediss://meada@cache.ejemplo.internal:6379"   # TLS fuera de loopback
password_file = "/run/secrets/valkey"                 # o password_env
key_prefix = "meada:prod"                             # por defecto "meada"

[generations]
keep = 3                                              # generaciones retenidas en la flota, al menos 2
```

## Puntero y retención

<details markdown="1" class="reference-details">
<summary>Retención en el store, poda local y rollback</summary>

`meada snapshot publish` termina escribiendo, en un solo script, `{<key_prefix>}:current` (un
hash `{id, fence}`) y `{<key_prefix>}:retained` (una lista de `<id>:<fence>`, de la más nueva a
la más antigua, cada id una vez, recortada a `keep`), y ya no escribe el `current` del store; el
fence es `0` sin el candado de refresh. Cada `meada snapshot pull` lee ambos de Valkey y
comprueba cada id (16 caracteres hexadecimales en minúscula) y cada fence (decimal) antes de
tocar el nodo; luego descarga toda generación retenida que el nodo no tiene y borra las que
salieron de la lista, nunca la actual ni la que acaba de reemplazar. El archivo de puntero del
nodo lleva el fence leído con el id. Una entrada malformada, un puntero ausente o una lista
vacía detienen el pull con un error que nombra la clave, y el nodo queda como estaba. La
generación actual se descarga y se activa primero: una generación retenida que falla después se
vuelve un aviso en stderr con el id y la causa, no bloquea nada, y el siguiente pull lo
reintenta. Use el mismo `keep` en el job y en los nodos. `meada status --generations` lee la
generación actual de Valkey y comprueba que el store guarda su documento; la línea
`store: unreachable (...)` dice si falló Valkey o el store.

</details>

## Fijar una generación

Un run de workflow fija los commits en que empezó con un pin en la ruta:
`/<identidad>@<generación>`, siendo la generación lo que sigue al último `@`, sirve esa
generación guardada en cualquier nodo mientras siga retenida, y `/<identidad>` sigue sirviendo la
actual. Una generación que el nodo no guarda responde 404 `text/plain` nombrando la generación
pedida, la actual y las guardadas. Cada respuesta HTTP nombra su generación en el encabezado
`Meada-Generation` (un 404 nombra la actual) y cada resultado de herramienta en su `_meta`,
`{"meada/generation": "<id>"}`: el run lee la generación de su primera respuesta y fija las
siguientes en ella. Las herramientas, los parámetros y el texto de las respuestas no cambian. El
proxy inverso reenvía `Meada-Generation` en las respuestas. Una llamada fijada aún abierta en
una generación que la retención de un pull borra puede fallar a mitad de camino.
