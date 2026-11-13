# Images and environment files

- `IMAGE_STORED_MAX` (`image/config.rs`): 4 times `lang::MAX_BYTES` per configuration file,
  bounding the image facts when a long job key is repeated for many tags. Past it further
  builder and direct-code facts are left out. Direct-code summaries charge node identity, root,
  path and reason before copying them. The reader shares the event/XML/Gradle depth and file ceilings;
  unresolved image patterns share `PATTERN_LITERAL_MIN`, `PATTERN_SEGMENTS` and `PATTERN_TEXT_MAX`.
  The orphan rows of `status` (`scope_status`) and the workspace's pattern pass
  (`patterns::prepare`) compare every image builder with every runner, with no index: 4,000
  patterns took 54.6 s for a `status` in a debug build.

- `image.rs`, `image/config.rs` (#833, ADR 0028): the builder forms of a pipeline. Read in a
  `run` script, through the command table's gate (`iac/actions/run.rs`): `docker push` with
  exactly one image (its flags `-a`, `--all-tags`, `-q`, `--quiet` and
  `--disable-content-trust`; the word after any other option is its value, so an unknown flag
  costs the image and gives no wrong one), `docker buildx build` with a literal `--push` and
  each `-t <image>`, `--tag <image>` and `--tag=<image>`, and `ko build` or `ko publish` with
  one `--bare`, at most one literal package and `KO_DOCKER_REPO` in an `env` of the document
  (read at index time, with no `env` literal in its words). Not read: `docker build` (also with
  `--push`), `docker image push`, `--push=true`, `--output type=registry`, `-o`, `docker buildx
  bake`, `docker compose push`, a global option before the subcommand, a prefix (`sudo`,
  `time`), a tag glued to `-t`, a word left open whole that may hold the options (`$ARGS`),
  Podman, Buildah, Kaniko, crane and Skopeo. A `buildx` word that reads `--push` or `-t` as
  the value of another option is taken for the option. `docker/build-push-action`,
  `docker/metadata-action` and `ko` are read in a pipeline file only, never in a composite
  action, while `docker push` and `docker buildx build` are read in both. A builder's site is
  its CI step when the CI reader gives the file that node, else the repository; the builders of
  Compose, Skaffold and Jib stay on the repository. A CI job is no workload: `container`,
  `services` and its `env` give no runner. A workload links to each CI step of a repository
  that builds its image and runs the repository's code once; `trace` down from the workload
  shows each such CI step. Of the sites of one repository that build one image, only those on
  its best evidence link (the level first, then a literal name before one that is evidence): a
  Compose service whose image is a default, beside a CI step that names the image, adds no
  link, and two CI steps on the same evidence both link. An image a script pushes under two
  tags is one side; `ko` and `docker push` of one image in one script are two rows. On a CI
  step `context` shows `builds <image>` and each runner linked to it, a pattern included.

- `PATHS_MAX`, `WORK_MAX` (`image/dockerfile.rs`) and `LIST_MAX` (`contract.rs`, #744): a Compose
  build runs the paths its Dockerfile takes from the context. A Dockerfile naming more than
  `PATHS_MAX` (256) sources, or matching more paths than that, is not read; the wildcard sources of
  one Dockerfile examine at most `WORK_MAX` (16,384) directory entries (a `*/*/*/*` source walks
  the tree, so past it the Dockerfile is not read); a directory past `LIST_MAX` (4,096) entries is
  not listed, so a wildcard through it leaves the Dockerfile unread. "Not read" is the fallback of
  ADR 0026: the whole context at `inferred`, the reason naming the cause. The fallback is also the
  answer for every form the reader does not parse: a source or mount with a variable, a quote or
  an escape, a mount type with a variable, a `**` source under `COPY --parents` (Docker's pattern
  matcher lets it cross directories), a `${...}` other than `${name}` on a line with `<<` (Docker
  splits it into words of its own), a source that climbs out of the context, an `# escape=`
  parser directive other than `\` (the other directives change nothing it reads), a Dockerfile
  behind a symlink or longer
  than `lang::MAX_BYTES`, a flag with a quoted value or one it does not list, an instruction it
  does not list, a heredoc with no end, and a Dockerfile outside the repository. A source that
  names nothing on disk is skipped (the build would fail), `.dockerignore` is not read (a path may
  cover files the image excludes, never fewer), `ONBUILD` instructions are ignored, and a path
  nested in another taken path is kept. The
  fallback's executions are stored in the room `IMAGE_STORED_MAX` leaves; past it a build gives
  none, as every other direct-code fact does.

- `ENVIRONMENT_BYTES_MAX` (`iac/environment.rs`, #663): one `lang::MAX_BYTES` (1 MiB) of
  environment expansion per workload, charging overwritten entries too so repeated `envFrom`
  references cannot multiply work without bound. Kustomize reference renames charge the same
  budget per workload and stage, before copying a transformed name. Past it the workload retains no environment
  entries, since a partial list could miss an override. Values beyond `VALUE_MAX` stay
  unresolved; Secret values stay absent. Compose charges repeated service names and images
  before copying them into entries; Kubernetes shares a container name while reading inputs.
  Unit/repository IaC budgets also charge these entries and multi-container images, typed
  targets, projected provisioned names and routing evidence. Lambda/SAM also charge repeated
  container/image strings before copying environment entries, including overridden entries.
  Lambda/SAM cap environment room after fixed image/code facts; ECS can discard an unread or
  oversized environment without discarding known images. Its direct JSON-encoder field scans
  spend `EVAL_WORK_MAX`, and computed encoded values keep the evaluator's `EVAL_VALUE_MAX`.
  Temporary owned map keys consume expansion room before cloning. Unknown SAM/Serverless
  map overrides keep known inherited variable names unread, rather than retaining old values.
