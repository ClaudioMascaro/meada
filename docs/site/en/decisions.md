# Decisions

An ambiguous link (several providers serve the route) or a dynamic URL (the call builds its URL
at run time) stays an open item. The MCP `search` tool, with the kind `ambiguous` or
`unresolved`, lists these items with how many candidates each has, not the candidates;
`meada decide --list` lists the items with their candidates. You resolve them in human mode, or
a model does if you ask.

- `meada decide` shows each open item of the repository, with numbered candidates; you pick a
  number, or `s` to skip. At the end it writes the picks to `meada.decisions.toml`, at the
  root. Nothing picked, nothing written.
- `--item <id>` works on that item only (the id `meada decide --list` or the `search` tool
  prints), even one already decided, to redo its decision; `--dry-run` prints the file that
  would be written and writes nothing; `--list` prints the open items plus the stale
  decisions and asks nothing; `--prune` removes the stale decisions (it does not combine with
  the previous three).
- `--decider` swaps the person for a model (see below); `--show-request` (only with
  `--decider`) prints every request that would be sent and sends none; `--min-p <p>` (only with
  `--decider`, from 0 to 1) is the least probability for the model to write a decision.

`meada.decisions.toml` is a file versioned in git, like a lockfile: its diff is what you
review. Each `[[decision]]` holds the call (`call`), the chosen provider (`target`), who decided
(`by`: `human` or the model id) and, for a model, the probability (`p`). Indexing only reads
the file back: the decided link gets the level `decided` and its reason says who decided. When
the code changes and a decision no longer fits (the call is gone, the item is no longer open,
the provider is gone or is no longer a candidate), `meada status` reports it as stale, under
its repository, and `meada decide --prune` removes it. A decision waiting for a peer that has
not been found yet is shown apart and kept.

### Model decider

`meada decide --decider` is the only path from Meada to a language model, and it runs only when you
ask. Four environment variables configure it (nothing comes from a file):

| Variable | Role | Default |
| --- | --- | --- |
| `MEADA_DECIDER_PROTOCOL` | `decisions` or `chat` | `decisions` |
| `MEADA_DECIDER_URL` | base URL | `https://openrouter.ai` (`decisions`); required with `chat` |
| `MEADA_DECIDER_KEY` | API key, sent only in the `Authorization` header | required with `decisions`; optional with `chat` |
| `MEADA_DECIDER_MODEL` | model | `typesafe/jev-1.14` (`decisions`); required with `chat` |

- **`decisions`** (the default) uses OpenRouter's decisions API, with Jev: a single request
  carries every item, and the response gives the probability of each candidate. The request
  asks for zero data retention (`zdr: true`, `data_collection: deny`); if OpenRouter refuses
  that preference, the request fails and nothing is written.
- **`chat`** talks to any endpoint compatible with OpenAI's chat API
  (`<URL>/chat/completions`), a local model included, for example through Ollama:
  `MEADA_DECIDER_PROTOCOL=chat MEADA_DECIDER_URL=http://localhost:11434/v1
  MEADA_DECIDER_MODEL=<model> meada decide --decider`. One request per item. The probability is
  the one the model reports about itself, not a calibrated measure.

Any gateway that speaks either protocol works: just change the URL. HTTPS anywhere; HTTP only
to `localhost` or a loopback address.

What leaves the machine, per item: the call, the function that contains it (read from disk) and
the list of candidates (repository, method and path), plus the id of the repository that makes
the call. No provider code, no other file. `--show-request` shows all of it first, with the key
replaced by `<hidden>`. Each answer is validated per item; an invalid one, or a request that
failed, is reported without writing that item, and the command exits with an error.

Fixed constants:

- `--min-p` defaults to 0.5: below it the proposal is printed as a suggestion and not written.
  It changes only through a PR with data.
- At most 50 candidates per item: with more, the item is skipped in decider mode (human mode
  still offers it).
- 120 seconds per request, from connection to last byte, with no retry.
- With the `decisions` protocol, the probabilities of an answer must sum to 1 within a
  tolerance of 0.01.
