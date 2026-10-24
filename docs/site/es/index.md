---
hide:
  - toc
---

# El mapa entre tus repositorios.

<div class="lead" markdown>
Meada lee el código, encuentra quién llama a quién y conecta repositorios mediante los contratos que comparten. Personas y agentes consultan el mismo mapa, sin LLM en la indexación.
</div>

## Empieza por tu código

Instala, ejecuta en la raíz de un repositorio y conecta tu agente.

```sh
cargo install meada
meada index
meada mcp
```

<div class="reading-paths" markdown>
- [Instalar Meada](installation.md)

    Cargo, binarios, npm y contenedores.

- [Conectar repositorios](configuration.md)

    Declara peers, hosts y recursos en meada.toml.

- [Consultar con tu agente](mcp.md)

    Encuentra símbolos, lee su contexto y sigue una llamada.

- [Operar para un equipo](server.md)

    Mirrors, generaciones, autenticación y consola.

</div>

<div class="tool-strip" markdown>
<div markdown>
[`search`](mcp.md)

Encuentra un punto de partida.
</div>
<div markdown>
[`context`](mcp.md)

Entiende las conexiones de un símbolo.
</div>
<div markdown>
[`trace`](mcp.md)

Sigue la cadena entre repositorios.
</div>
</div>

## Qué está disponible

El modo servidor ([S1, #259](https://github.com/ClaudioMascaro/meada/issues/259)) y la consola ([S2, #293](https://github.com/ClaudioMascaro/meada/issues/293)) están entregados. Ownership por equipo (S3), automatización de identidad (S4) y workspaces con nombre (S5) están planificados. Cada página distingue el comportamiento disponible de la propuesta.

[Consulta las capacidades y los límites](capabilities.md) · [S3](workspaces.md) · [S4](enterprise.md) · [S5](named-workspaces.md)
