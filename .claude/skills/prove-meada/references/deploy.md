# Deploy

These product execution commands are CI-only recipes under `AGENTS.md`. Locally, prepare
inputs or read the matching run artifact; do not build or start the product.


## Renders, base against head

```bash
python3 $S/prove.py deploy --base "$BASE" --run "$RUN"
```

It renders, at the base (a detached worktree under `$RUN`, removed after) and in this working
tree: the Compose profiles `scripts/compose-check` checks (`local`, `generations`, `console`,
`postgres` and the accounts stack of `.env.accounts.example`) with `docker compose ... config
--format json`, and with `helm` on `PATH` the chart once per `deploy/helm/meada/values*.yaml`.
Paths are normalised to `<root>`. The files the stack mounts (`Caddyfile*`, `*.toml`, `*.sh`,
`.env*`) are copied beside the renders as `mounted-<name>`, since a render carries only their
path. Each render and each mounted file is compared line by line and every difference shows in
`$RUN/deploy.md`. A render that fails at the head is a difference of its own. Without `docker`
the Compose renders are Not covered (a note in `deploy.md`), with `helm` missing the chart is.

Every difference must be one the issue names (a new service, a changed limit, a version bump in a
release PR). An image tag that moves outside a release PR is BROKEN.

## The repository's own checks

```bash
python3 scripts/compose-check   # docker compose, no container starts
python3 scripts/helm-check      # helm and PyYAML, no cluster
```

Both must exit 0. They hold the contract the deployment docs promise (profiles without a console,
the console's command, the job order of the managed refresh); a render diff says what moved,
these say whether the contract still holds.

## Containers

Only when Scope reaches `Dockerfile`, `scripts/container-smoke` or `scripts/ui-tarball`, and only
with Docker:

```bash
docker build -t meada:zz-prove .
python3 scripts/container-smoke meada:zz-prove
docker image rm meada:zz-prove
```

The image build compiles a release binary inside Docker: minutes and gigabytes. Without Docker
(the cloud sandbox) it is BLOCKED here and CI's `container.yml` runs it on the PR; list it under
Not covered with that unlock.

## Not covered

- A cluster or a running Compose stack: neither check starts one. The unlock is
  `docker compose --profile local up` in `deploy/compose` (see its README) on a machine with
  Docker and a few GB free.
