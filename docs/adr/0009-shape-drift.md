# 9. Shape drift

- Status: accepted
- Date: 2026-10-14

## Context

A link that holds can still break: the provider stops returning a field that the consumer reads.
Prior art does not report it (`docs/prior-art.md`, gap 3). Many repositories have no OpenAPI
document, so comparing only against one would miss them. In the maintainer's private dogfood
repositories, the provider validates with zod and the consumers mirror the response by hand.

## Decision

Shape drift is in v1 and ships in track A (ADR 0003).

- The provider shape comes from OpenAPI documents, zod schemas and the object literals that
  route handlers return.
- The consumer shape comes from the field reads on the parsed response and from the types used
  in casts.
- Meada reports each field a consumer reads that the provider no longer returns. The report
  carries a level (ADR 0005).
- TypeScript and JavaScript come first. Other languages are covered through OpenAPI and as
  their language packs arrive (ADR 0007).

## Considered options

- **OpenAPI-only comparison.** Little value for repositories without OpenAPI.
- **Deferring past v1.** Shape drift is one of the differences Meada claims over prior art.
