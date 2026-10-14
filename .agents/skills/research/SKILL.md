---
name: research
description: Investigate a question against high-trust primary sources and capture the findings as a Markdown file in the repo. Use when the user wants a topic researched, docs or API facts gathered, or reading legwork delegated to a background agent.
---

Spin up a **background agent** to do the research, so you keep working while it reads.

Its job:

1. Investigate the question against **primary sources** (official docs, source code, specs, first-party APIs), not a secondary write-up of them. Follow every claim back to the source that owns it.
2. Write the findings to a single Markdown file, citing each claim's source.
3. Save it where the repo already keeps such notes; match the existing convention, and if there is none, put it somewhere sensible and say where. Write it in the language the target repo's `CLAUDE.md` sets for that folder (the platform's `docs/research/` is pt-BR; most else is English).
4. In plan mode the repo is read-only: write the file to `$CLAUDE_JOB_DIR/tmp/` (or the session's plans dir), name the intended repo path in the report, and the coordinator moves it there after approval, in one commit.
