# The `meada` server on this repository

How the code graph `AGENTS.md` names stays current, and which tree it answers about.

- Install the binary once per machine with `cargo install --path . --locked`, and again after a
  change to a language pack or a contract kind: the server runs the installed binary, while the
  index follows the working tree on every query. After a kind change, also delete `.meada/`: a
  refresh keeps the rows of the language groups no file changed in while Meada's version holds.
- The server answers about the first MCP root the client sends (Claude Code sends the session's
  directory first), so a session in a worktree under `.claude/worktrees/` gets its own tree even
  though the desktop app starts the server in the main checkout. A client without roots gets the
  directory the server started in. Subagents share their parent's server and its roots (#192).
