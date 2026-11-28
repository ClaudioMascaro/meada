# Reading GitHub Actions commands

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Overview

Breaks down a `run` step’s text for package and image extractors.

## Interface

`commands` takes the script and a function for looking up the step environment. It returns
commands as lists of words with literal or open parts.
[run.rs:commands](./run.rs) [run.rs:Part](./run.rs)

`environment_file` distinguishes a script with no reference to `GITHUB_ENV`, writes whose names
are visible in `echo NAME=...`, and writes that may introduce unknown names.
[run.rs:environment_file](./run.rs)

## Invariants

A heredoc or unclosed delimiter prevents parsing. A variable is substituted only if its value is
simple, fits the budget and the script preserves its binding.
[run.rs:commands](./run.rs) [run.rs:rebound](./run.rs)

## Side effects

The parser calls the environment lookup and keeps its budget and words in memory. Shell text is
input for static analysis.
[run.rs:commands](./run.rs)

## Verification

```sh
bounded cargo test --locked -p meada --lib --test it -- iac::actions ci_run::
```
