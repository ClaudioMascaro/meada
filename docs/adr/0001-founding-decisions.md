# 1. Founding decisions

- Status: accepted
- Date: 2026-10-14

## Context

GitNexus showed that an LLM-free code graph served over MCP helps coding agents navigate code,
and its "groups" link repositories through the contracts they share. Its PolyForm
Noncommercial license rules it out for commercial work. MIT and Apache-2.0 projects cover parts
of the same ground (CodeGraph, codebase-memory-mcp, kivgraph). Meada starts anyway, as a new
project with two emphases: cross-repo context as the headline feature, and an open-source
practice rooted in Brazil and Latin America.

## Decision

1. **Name**: Meada, from the Portuguese idiom "puxar o fio da meada", to find the thread that
   untangles the skein.
2. **License**: Apache-2.0: permissive, with an explicit patent grant from contributors.
3. **Implementation language**: Rust (edition 2024), for a single static binary and fast
   indexing; tree-sitter for parsing.
4. **Documentation languages**: Brazilian Portuguese is the source language of prose meant for
   people; English and Spanish translations follow, in that order. Code, comments, commits,
   ADRs and specs are in English so that anyone can read the code.
5. **Clean room**: no GitNexus code or tuned constants. Its behavior is described in
   the maintainer's notes, kept outside the repository; Meada's specs and ADRs are the only
   input to implementation.
6. **Hosting**: `github.com/ClaudioMascaro/meada`, transferable to an organization later.

## Consequences

- Fewer Brazilian developers know Rust than TypeScript; the pt-BR-first docs and well-scoped
  first issues carry the weight of lowering that barrier.
- Every pt-BR doc change carries translation work, tracked as `translation` issues when it
  cannot land in the same PR.
- Prior-art projects are ahead on language coverage; Meada competes on cross-repo depth and on
  its working principles (ADR 0002).
