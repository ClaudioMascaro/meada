# Agentes (MCP)

`meada mcp` habla MCP por stdio. Responde sobre la primera raíz (`roots`) que el cliente envía,
en Claude Code la carpeta de la sesión; sin raíces, sobre la carpeta en la que se inició, que
entonces debe estar dentro del repositorio. Si el cliente falla o no responde a `roots/list` en 5 s,
la llamada usa la última raíz que envió en esa conexión, o devuelve un error que lo dice; nunca
recurre a la carpeta del servidor por eso (que, en una sesión en un worktree, sería otro
checkout). Una respuesta sin raíz local vale como sin raíces (la carpeta del servidor). En la
configuración de tu agente (por ejemplo el `.mcp.json` de Claude Code), en la forma de tu canal:

=== "npx"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "npx", "args": ["-y", "@meada/meada", "mcp"] }
      }
    }
    ```

=== "meada en el PATH"

    ```json
    {
      "mcpServers": {
        "meada": { "command": "meada", "args": ["mcp"] }
      }
    }
    ```

=== "Docker"

    ```json
    {
      "mcpServers": {
        "meada": {
          "command": "sh",
          "args": ["-c", "exec docker run -i --rm --network none --user \"$(id -u):$(id -g)\" -v \"${PWD%/*}:${PWD%/*}\" -w \"$PWD\" ghcr.io/claudiomascaro/meada:1.0.0 mcp"]
        }
      }
    }
    ```

    El cliente inicia el servidor en el repositorio; el contenedor monta la carpeta que lo contiene,
    y sus pares vecinos, en la misma ruta de la raíz que envía el cliente, y corre con tu usuario y
    sin red. Solo Linux y macOS.

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

## Skill para agentes

La skill [`skills/meada/SKILL.md`](https://github.com/ClaudioMascaro/meada/blob/main/skills/meada/SKILL.md)
le enseña al agente a elegir entre `search`, `context` y `trace` y a leer los niveles de una
respuesta: `inferred` y `ambiguous` son coincidencias de nombre, y un llamador ausente no está
probado. Vale para la versión con la que se publicó; descarga la de tu versión.

- **Claude Code**: cópiala a `~/.claude/skills/meada/` (todas las sesiones) o a
  `.claude/skills/meada/` de un proyecto:

    ```sh
    mkdir -p ~/.claude/skills/meada
    curl -fsSL https://raw.githubusercontent.com/ClaudioMascaro/meada/v1.0.0/skills/meada/SKILL.md -o ~/.claude/skills/meada/SKILL.md
    ```

- **Otro agente que lee carpetas de skills** (`SKILL.md` con frontmatter): copia la carpeta
  `skills/meada/` a su carpeta de skills.
- **Un agente sin skills**: pega el cuerpo de `SKILL.md`, sin el frontmatter, en el `AGENTS.md`
  del proyecto.
