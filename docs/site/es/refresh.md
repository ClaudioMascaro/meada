# Refresh y publicación

La política elegida es publicar el conjunto completo o no publicar nada: cualquier fallo de fetch o indexación debe abortar la publicación y conservar la generación actual. La implementación y verificación de esa garantía están [planeadas en #316](https://github.com/ClaudioMascaro/meada/issues/316). Hoy, el operador puede encadenar el job con `sh -ec` para detenerse ante un fallo; lock y fence ya protegen el puntero. En [S4 planeado](enterprise.md), las exclusiones por elegibilidad serán decisiones exitosas previas al fetch.

<!-- meada-graph:refresh -->

**Candado de refresh** (feature `refresh-lock`, que incluye `generation-pointer`, solo Unix;
`cargo install meada --features refresh-lock`). Con la sección `[refresh_lock]`, que exige
`[generations]`, todo el refresh corre bajo un candado en Valkey, y cada adquisición recibe un
fence, de modo que un job superpuesto o trabado nunca reemplaza una generación nueva por una más
antigua.

```toml
[refresh_lock]
ttl_seconds = 300                                     # por defecto; al menos 10
```

```sh
meada snapshot lock --workspace ops/meada.workspace.toml -- \
  sh -c 'git -C api fetch && git -C api reset --hard origin/main &&
         meada index --workspace ops/meada.workspace.toml &&
         meada snapshot publish --workspace ops/meada.workspace.toml'
```

<details markdown="1" class="reference-details">
<summary>Adquisición, renovación y pérdida del bloqueo</summary>

`meada snapshot lock` toma el candado con `SET {<key_prefix>}:lock <token aleatorio> NX PX
<ttl>`, suma uno a `{<key_prefix>}:fence` y corre el comando como hijo, con `MEADA_REFRESH_LOCK`
(el token) y `MEADA_REFRESH_FENCE` en el entorno. El candado se renueva cada tercio del TTL
(comparar y extender) y se libera al final (comparar y borrar); el código de salida es el del
hijo (128 más la señal cuando una señal lo terminó). Un candado ya tomado sale con 1 nombrando la
clave, sin correr nada. Cuando una renovación encuentra otro token, o ninguna tuvo éxito durante
un TTL entero, el candado está perdido: el grupo de procesos del hijo recibe SIGTERM (y SIGKILL
10 s después si sigue vivo), y `lock` sale con 1 diciendo que lo perdió. Un SIGTERM, SIGINT, SIGHUP o
SIGQUIT a `lock` llega al grupo del mismo modo; cuando el hijo termina, lo que quedó en su grupo
de procesos recibe SIGKILL (un nieto que llama a `setsid` sale del grupo y escapa), y en Linux el hijo recibe SIGTERM si `lock` muere antes. La indexación sigue sin red:
el `meada index` del hijo no abre ninguna conexión.

</details>

## Publicación y recuperación del fence

<!-- meada-graph:fence-recovery -->

Con `[refresh_lock]`, `meada snapshot publish` exige las dos variables, y antes de cada envío
comprueba que el candado aún guarda el token: si no, se detiene sin enviar nada más y sin
escribir el puntero. El script del puntero solo escribe cuando el candado guarda el token y el
fence es mayor que el de `{<key_prefix>}:current`, en una sola operación. Los nodos nunca pasan
a una generación con fence menor que el de su archivo de puntero; sin el archivo, toman la
actual sea cual sea su fence. Meada nunca baja ni borra `{<key_prefix>}:fence`. Si un flush o un
failover de Valkey lo pierde, los fences siguientes quedan por debajo de los registrados y la
flota se detiene: el publish rechazado sale con 1 nombrando `{<key_prefix>}:fence`, el pull de un
nodo con fence mayor sale con 1 sin cambiar, y `meada status --generations` en ese nodo imprime
una línea `fence: stuck: ...` con los dos fences (su código de salida no cambia). Ejecute
`meada snapshot fence recover --workspace <manifiesto>`: bajo el lock, eleva el contador hasta
el mayor fence del pointer, las generaciones retenidas, el workspace y los heartbeats válidos,
usando el reloj de Valkey para seleccionar los nodos vivos. El siguiente publish usa un fence
mayor y desbloquea los nodos. `INCR` de Valkey tiene un límite de 9223372036854775807; la
recuperación rechaza pisos o fences registrados superiores a 9223372036854775806 y no consume
la última adquisición disponible. La lectura y el transporte siguen aceptando fences `u64` exactos. Para un nodo aislado cuyo fence
no aparece en un heartbeat válido, páselo como `--at-least <fence>`, o borre su
`<local_dir>/current` para que tome la generación actual. Apagar `[refresh_lock]` después de que los nodos registraron fences
por encima de 0 también detiene la flota, ya que sin el candado todo fence es 0: borre el
`<local_dir>/current` de cada nodo.
