# Decisiones

Un enlace ambiguo (varios proveedores sirven la ruta) o una URL dinámica (la llamada arma la URL
en tiempo de ejecución) queda como ítem abierto. La herramienta `search` del MCP, con el tipo
`ambiguous` o `unresolved`, lista esos ítems con cuántos candidatos tiene cada uno, no los
candidatos; `meada decide --list` lista los ítems con sus candidatos. Los resuelves tú, en el
modo humano, o un modelo, si lo pides.

- `meada decide` muestra cada ítem abierto del repositorio, con los candidatos numerados; eliges
  un número, o `s` para saltar. Al final graba las elecciones en `meada.decisions.toml`, en la
  raíz. Si no elegiste nada, no graba.
- `--item <id>` trata solo ese ítem (el id que imprime `meada decide --list` o la herramienta
  `search`), aunque ya esté decidido, para rehacer su decisión; `--dry-run` imprime el archivo
  que se grabaría y no escribe; `--list` imprime los ítems abiertos, más las decisiones
  obsoletas, y no pregunta nada; `--prune` quita las decisiones obsoletas (no se combina con
  las tres anteriores).
- `--decider` cambia la persona por un modelo (ver abajo); `--show-request` (solo con
  `--decider`) imprime cada solicitud que se enviaría y no envía ninguna; `--min-p <p>` (solo
  con `--decider`, de 0 a 1) es la probabilidad mínima para que el modelo grabe una decisión.

`meada.decisions.toml` es un archivo versionado en git, como un lockfile: su diff es lo que se
revisa. Cada `[[decision]]` trae la llamada (`call`), el proveedor elegido (`target`), quién
decidió (`by`: `human` o el id del modelo) y, para un modelo, la probabilidad (`p`). La
indexación solo vuelve a leer el archivo: el enlace decidido recibe el nivel `decided` y su
motivo dice quién decidió. Cuando el código cambia y una decisión deja de encajar (la llamada
desapareció, el ítem ya no está abierto, el proveedor desapareció o ya no es candidato),
`meada status` la muestra como obsoleta, bajo su repositorio, y `meada decide --prune` la quita.
Una decisión que espera un par aún no encontrado aparece aparte y se conserva.

### Decisor por modelo

`meada decide --decider` es el único camino de Meada hacia un modelo de lenguaje, y solo corre cuando lo
pides. Cuatro variables de entorno lo configuran (nada viene de un archivo):

| Variable | Papel | Valor por defecto |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` o `chat` | `decisions` |
| `MEADA_DECIDER_URL` | URL base | `https://openrouter.ai` (`decisions`); obligatoria con `chat` |
| `MEADA_DECIDER_KEY` | clave de la API, enviada solo en el encabezado `Authorization` | obligatoria con `decisions`; opcional con `chat` |
| `MEADA_DECIDER_MODEL` | modelo | `typesafe/jev-1.14` (`decisions`); obligatoria con `chat` |

- **`decisions`** (el predeterminado) usa la API de decisiones de OpenRouter, con Jev: una sola
  solicitud lleva todos los ítems, y la respuesta trae la probabilidad de cada candidato. La
  solicitud pide cero retención de datos (`zdr: true`, `data_collection: deny`); si OpenRouter
  rechaza esa preferencia, la solicitud falla y no se graba nada.
- **`chat`** habla con cualquier endpoint compatible con la API de chat de OpenAI
  (`<URL>/chat/completions`), incluso un modelo local, por ejemplo con Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<modelo> meada decide --decider`. Una solicitud por ítem. La probabilidad
  es la que el propio modelo dice de sí, no una medida calibrada.

Sirve cualquier gateway que hable uno de los dos protocolos: basta cambiar la URL. HTTPS en
cualquier lugar; HTTP solo hacia `localhost` o una dirección de loopback.

Lo que sale de la máquina, por ítem: la llamada, la función que la contiene (leída del disco) y
la lista de candidatos (repositorio, método y ruta), más el id del repositorio que hace la
llamada. Nada de código del proveedor, ningún otro archivo. `--show-request` muestra todo eso
antes, con la clave cambiada por `<hidden>`. Cada respuesta se valida por ítem; una inválida, o
una solicitud que falló, se informa sin grabar ese ítem, y el comando termina con error.

Constantes fijas:

- `--min-p` vale 0,5 por defecto: por debajo, la propuesta se imprime como sugerencia y no se
  graba. Cambia solo mediante un PR con datos.
- Como máximo 50 candidatos por ítem: con más, el ítem se salta en el modo decisor (el modo
  humano aún lo ofrece).
- 120 segundos por solicitud, de la conexión al último byte, sin reintento.
- Con el protocolo `decisions`, las probabilidades de una respuesta deben sumar 1 con una
  tolerancia de 0,01.
