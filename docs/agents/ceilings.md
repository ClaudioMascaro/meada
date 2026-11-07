# Ceilings of the code

Every limit of the code that `CODING_STANDARDS.md` asks for (hostile input) and every other ceiling
a ticket accepted, each with its value and its reason. Detail behind the
[architecture map](architecture.md); the code wins over this file. A limit marked `ponytail:` in
`src/` is named in one of the parts below, and `tests/it/docs.rs` checks that it is.

- [Resolution, the index and shape readers](ceilings/resolution-the-index-and-shape-readers.md)
- [Language packs](ceilings/language-packs.md)
- [HTTP routes and clients (JVM, Go, Rust, TS)](ceilings/http-clients-jvm-go-rust-ts.md)
- [HTTP routes and clients (C# and Swift)](ceilings/http-clients-csharp-swift.md)
- [Channel clients](ceilings/channel-clients.md)
- [Server, accounts, workspaces and deploy](ceilings/server-accounts-workspaces-and-deploy.md)

- [Azure SDK clients](ceilings/azure-sdk-clients.md): bounded five-group inventory, shared evaluation controls and scoped receiver names.
