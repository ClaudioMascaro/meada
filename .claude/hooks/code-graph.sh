#!/bin/sh
# SessionStart hook after a compaction: the summary keeps what was done, not how it was found,
# and the `meada` MCP tool schemas come back deferred. One line puts the code graph back in reach.
cat <<'TEXT'
Code graph: the `meada` MCP server indexes this repository. Ask it first where a symbol lives (`search`), who calls it and what it holds (`context`) and what a change reaches (`trace`), then read the files it names. When the `mcp__meada__*` schemas are deferred, load them with ToolSearch: `select:mcp__meada__search,mcp__meada__context,mcp__meada__trace`. What it does not cover is in AGENTS.md, "Code graph".
TEXT
