# Azure SDK calls by language

Azure SDK call forms in C#, Go, Java, Python and JS/TS.

## Interface

Each reader recognizes imported constructors, follows child clients and classifies the final operation. The environment reader combines the parts of the receiver identity. `[csharp.rs:constructed]` `[go.rs:step]` `[java.rs:operation]` `[python.rs:read]` `[ts.rs:read]` `[environment.rs:environment_identity]`

## Invariants

When both a Service Bus topic and its subscription come from environment variables, their names must be unambiguous in the same execution scope. Literals can be used directly. `[environment.rs:environment_identity]`

When combining values in one scope, the text must be nonempty and all occurrences must also agree on whether the value is a typed reference. `[environment.rs:unique]`

## Side Effects

Analysis operates on previously collected calls and values in memory. `[ts.rs:read]` `[environment.rs:environment_identity]`

## Verification

From the repository root, run `cargo test -p meada --test it azure_sdk::`.
