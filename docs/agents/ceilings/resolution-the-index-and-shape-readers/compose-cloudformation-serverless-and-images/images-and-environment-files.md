# Images and environment files

- `IMAGE_STORED_MAX` (`image/config.rs`): 4 times `lang::MAX_BYTES` per configuration file,
  bounding the image facts when a long job key is repeated for many tags. Past it further
  builder and direct-code facts are left out. Direct-code summaries charge node identity, root,
  path and reason before copying them. The reader shares the event/XML/Gradle depth and file ceilings;
  unresolved image patterns share `PATTERN_LITERAL_MIN`, `PATTERN_SEGMENTS` and `PATTERN_TEXT_MAX`.
  The orphan rows of `status` (`scope_status`) and the workspace's pattern pass
  (`patterns::prepare`) compare every image builder with every runner, with no index: 4,000
  patterns took 54.6 s for a `status` in a debug build.

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
