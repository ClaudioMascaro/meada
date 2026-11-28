# Maintenance tools

Local branch checks, deployment profile validation and tools used by the tests.

## Interface

`scripts/check` selects checks for the change scope. The regular expression oracle takes a JSON array and tests each pattern with the Go engine. `[check:main]` `[regexp-oracle.go:main]`

## Invariants

The oracle returns two results per pattern in input order. A successful compilation produces an empty string. `[regexp-oracle.go:main]`

## Side effects

The checker starts build and test processes. The oracle writes to stdout or the file passed as its second argument. `[check:bounded]` `[regexp-oracle.go:main]`

## Components

- [Site build](./docs/README.en.md)

## Verification

From the repository root, run `scripts/check --selftest`.
