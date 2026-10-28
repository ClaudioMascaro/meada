# Agentes (MCP)

`meada mcp` habla MCP por stdio. Responde sobre la primera raíz (`roots`) que el cliente envía,
en Claude Code la carpeta de la sesión; sin raíces, sobre la carpeta en la que se inició, que
entonces debe estar dentro del repositorio. Si el cliente falla o no responde a `roots/list` en 5 s,
la llamada usa la última raíz que envió en esa conexión, o devuelve un error que lo dice; nunca
recurre a la carpeta del servidor por eso (que, en una sesión en un worktree, sería otro
checkout). Una respuesta sin raíz local vale como sin raíces (la carpeta del servidor). En la
configuración de tu agente (por ejemplo el `.mcp.json` de Claude Code), con `npx`:

```json
{
  "mcpServers": {
    "meada": { "command": "npx", "args": ["meada", "mcp"] }
  }
}
```

Con `meada` ya en el `PATH`, usa `"command": "meada", "args": ["mcp"]`.

Son tres herramientas, siempre en inglés, y ninguna escribe:

- `search` encuentra un símbolo, una ruta o un contrato por el nombre; con el tipo `resource`,
  un recurso por su tipo o su id; con el tipo `channel`, un canal (cola o tópico) por el broker
  o el nombre; con el tipo `duplicate`, grupos de símbolos con texto idéntico en dos o más lugares;
  con el tipo `ambiguous` o `unresolved`, lista los ítems
  abiertos (ver Decisiones);
- `context` describe un símbolo: su código leído del disco, quién lo llama, a quién llama y los
  contratos que toca; las anotaciones, atributos y decoradores escritos sobre una definición
  (`@Override`, `[Fact]`, `@app.get("/x")`) son llamadas del ámbito que la envuelve, por eso
  `context` las lista aparte, en la sección `annotations:`, cada una con la definición que
  decora (el sitio de llamada del export lo trae en `decorates`); en un recurso (`resource::postgres::orders`), lista quién lo escribe y
  quién lo lee, con el símbolo y la línea, o `(repository-level)` para el repositorio entero; en
  un canal (`channel::kafka::orders.created`), lista productores y consumidores, también los
  que llegan a él por un comodín o por un binding, con el nivel y el motivo, y un lado cuyo
  nombre Meada no puede evaluar aparece como `(unresolved)` y nunca se enlaza; los candidatos de una llamada ambigua que
  son sobrecargas de un método (un archivo, una clase, un nombre) aparecen como un conjunto,
  `(one of N overloads)`, y `trace` los escribe como `Class.name (N overloads)`; la llamada
  sigue `ambiguous` y el export lista cada candidato como antes;
- `trace` sigue una cadena de llamadas por varios repositorios, río abajo (`down`) o río arriba
  (`up`); con `changes: uncommitted`, parte del diff del árbol de trabajo y sirve para analizar
  el impacto de lo que se acaba de editar.

Las respuestas son líneas compactas dentro de un presupuesto de tokens (2.000 por defecto,
ajustable en cada llamada). Cada enlace entre repositorios lleva el nivel y el motivo
(`→[level: reason]`), y `trace` termina con un pie: el nivel más débil de la cadena, el número
de saltos, los repositorios involucrados y los tokens usados.
