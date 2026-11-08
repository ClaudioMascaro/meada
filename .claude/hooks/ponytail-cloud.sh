#!/bin/sh
# Runs one of ponytail's own hook scripts (hooks/<name>.js) from the installed plugin. Cloud
# sessions do not fire the plugin's hooks, so the project settings call them; local sessions fire
# them from the plugin, so this stays off there.
# shortcut: if the cloud starts firing the plugin's hooks, both run; drop these entries then.
[ "$CLAUDE_CODE_REMOTE" = true ] || exit 0
root=$(ls -d "${CLAUDE_CONFIG_DIR:-$HOME/.claude}"/plugins/cache/ponytail/ponytail/*/ 2>/dev/null | sort -V | tail -n 1)
[ -n "$root" ] || exit 0
CLAUDE_PLUGIN_ROOT=${root%/} exec node "${root}hooks/$1"
