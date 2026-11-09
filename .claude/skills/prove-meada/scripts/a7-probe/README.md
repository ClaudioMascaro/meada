The A7 probe repository of prove-meada: `prove.py values` copies this folder into a repository of
its own (`a7-probe` in `infra.tsv`), adds `zz-gitignored.yaml` (listed in `.gitignore`, so it is
on disk but never committed) and indexes it. Every file is here for one of #642's user stories 6
to 13; the oracle in `prove.py` reads them on its own and never trusts Meada's answer.
