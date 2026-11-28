# Azure SDK extraction

Turns Azure SDK calls into channel or resource contract sides.

## Interface

The policy carries client identity to the Service Bus, Event Hubs, Blob Storage or Key Vault operation and returns results for the requested family. `[sdk.rs:extract]`

## Invariants

Extraction requires receiver evidence. A constructor or method name alone cannot establish the direction of a contract. `[sdk.rs:extract]`

## Side Effects

Accumulates the sides found in memory from each file’s facts. `[sdk.rs:extract]`

## Components

- [Azure SDK calls by language](./sdk/README.en.md)

## Verification

From the repository root, run `cargo test -p meada --test it azure_sdk::`.
