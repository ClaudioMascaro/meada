#!/bin/sh
# SessionStart hook after a compaction: the summary keeps what was done, not how it was found,
# and the `meada` MCP tool schemas come back deferred. One line puts the code graph back in reach.
cat <<'TEXT'
Code graph: the `meada` MCP server indexes this repository. The trigger: "who calls X", "what does X call or hold" or "what does a change to X reach" goes to `context` or `trace` on X (`search` when only part of the name is known) before a grep or the read of a file to answer it. When the `mcp__meada__*` schemas are deferred, load them with ToolSearch: `select:mcp__meada__search,mcp__meada__context,mcp__meada__trace`. What it does not cover is in AGENTS.md, "Code graph".
TEXT
