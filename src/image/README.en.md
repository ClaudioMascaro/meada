# Container images and executed code

Links image builders to the workloads running their images and the local code included in the build.

## Interface

The reader summarizes build configuration, including Jib destinations and Dockerfile inputs. Preparation resolves image-name patterns and environment projection preserves the container running the code. `[config.rs:read]` `[jib.rs:gradle]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]` `[environment.rs:scoped_values]`

With `[[helm_env]]`, the reader extracts scalar entries at the declared key in charts containing `Chart.yaml`. Each `values-<env>.yaml` or `.yml` overrides the base values. With no variants, the base forms one environment. `[config.rs:chart_envs]` `[config.rs:laid]`

## Invariants

When Dockerfile inputs cannot be determined, the reader can attribute the entire context as inferred evidence. Verified code paths stay inside the repository. `[dockerfile.rs:executions]` `[code.rs:checked]`

Only the package that owns the chart receives that environment for channel resolution. When every environment of every chart that sets the variable agrees on a literal, the evidence can reach `declared`. A missing value in one of those environments or differing values cap results at `inferred`. `[environment.rs:entries]` `[environment.rs:values]` `[environment.rs:resolve]`

## Side Effects

Indexing checks paths in the checkout and reads the Dockerfile. Preparing image candidates updates exports in memory. `[code.rs:checked]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]`

The reader lists chart files and reads the base from the checkout. Exceeding the file budget skips the builder being read or drops the entire Helm environment. `[config.rs:chart_envs]` `[config.rs:builder]` `[config.rs:IMAGE_STORED_MAX]`

## Verification

From the repository root, run `cargo test -p meada --test it -- images:: helm_env::`.
