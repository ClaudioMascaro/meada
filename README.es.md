# Meada

[Português (Brasil)](README.md) · [English](README.en.md) · **Español**

> Esta es una traducción. El [README](README.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Meada lee tus repositorios, arma el mapa de quién llama a quién y cose un repositorio con otro
a través de los contratos que comparten: rutas HTTP, colas, paquetes, recursos compartidos. Los
agentes de código, y también las personas, consultan ese mapa por MCP. Ninguna etapa de la
indexación usa un LLM.

> **Estado:** en construcción. Todavía no hay nada para instalar. Las decisiones tomadas hasta
> ahora están en [`docs/adr`](docs/adr).

## Por qué otra herramienta

Quien trabaja con varios servicios sabe que el bug casi nunca vive en un solo repositorio. El
front llama a una ruta que cambió en el back; el worker consume una cola que otro equipo
renombró. De las herramientas de grafo de código que existen hoy, algunas miran un repositorio
a la vez, otras solo conectan repositorios cuando el texto de la ruta coincide exactamente, y
otras prohíben el uso comercial.

Meada quiere seguir el hilo de punta a punta, atravesando todos los repositorios que haga falta,
y decir con qué confianza se hizo cada conexión.

## Principios

- **Tu código se queda contigo.** Corre en tu máquina, sin cuenta, sin clave de API y sin
  telemetría.
- **Sin LLM para indexar.** El mapa sale del árbol sintáctico del código. No hay factura en
  dólares por token, funciona sin conexión y cabe en una laptop común.
- **Determinista.** El mismo código con la misma configuración siempre da el mismo mapa.
  Cuando una conexión es ambigua, la decisión queda en un archivo versionado en git, tomada
  por una persona o, si lo prefieres, por un modelo de lenguaje en un comando aparte. La
  indexación solo vuelve a leer esa decisión.
- **Portugués primero.** La documentación nace en portugués de Brasil; inglés y español son
  traducciones. Las issues y los PR pueden abrirse en cualquiera de los tres idiomas. El código
  se escribe en inglés para que cualquier persona pueda leerlo.
- **Juntar sin mezclar.** "No todo lo que se junta se mezcla", enseñó Antônio Bispo dos Santos,
  conocido como Nêgo Bispo. Cada repositorio guarda su propio índice; Meada solo cose las
  puntas.
- **Poco ruido para el agente.** Meada no escribe en tu `CLAUDE.md`, no instala hooks ni toca
  la configuración de tu editor sin un comando tuyo. Las respuestas son cortas.
- **Crédito a quienes vinieron antes.** Meada aprende de GitNexus, CodeGraph,
  codebase-memory-mcp y kivgraph.

## Cómo participar

El proyecto está empezando. El alcance de la v1 y el orden del trabajo están en el
[ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md). Ya puedes ayudar:

- abriendo una issue con tu caso de uso entre repositorios, en portugués, español o inglés;
- revisando las traducciones: si el inglés o el español suenan raros, la corrección es
  bienvenida;
- leyendo y cuestionando los ADR.

## El nombre

*Meada* es una madeja de hilo. *Puxar o fio da meada*, "tirar del hilo de la madeja", es
encontrar la punta que deshace el nudo, y eso es lo que intenta hacer la herramienta cuando
sigue una llamada de un repositorio a otro.

## Licencia

[Apache-2.0](LICENSE).
