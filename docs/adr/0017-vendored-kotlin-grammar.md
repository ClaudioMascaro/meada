# 17. The Kotlin grammar is vendored, with two upstream fixes

- Status: accepted
- Date: 2026-10-25

## Context

The Kotlin pack (spec #27) parsed with the `tree-sitter-kotlin-ng` 1.1.0 crate. That grammar
silently misparses common Kotlin: a class under two annotations, with no constructor and a
one-line `companion object { .. }` body, becomes an infix call (`class` as an operand, the class
name as the infix function), with no ERROR node, so the class, its members and its Spring
mappings vanish from the index. On a corpus of 1,691 files (Exposed, spring-framework,
ktor-samples, spring-petclinic-kotlin) 18 files lost declarations this way, among them
`@Configuration open class`, `@Service class` and annotated `annotation class`. Two members on
one line (`class C { fun a() {} fun b() {} }`) end in an ERROR node.

Upstream (`tree-sitter-grammars/tree-sitter-kotlin`, MIT) has open, unmerged fixes: PR #21
(`prec.dynamic(-10)` on `annotated_expression`) and PR #13, which carries PR #11 (the external
scanner ends a class member before `}` or a declaration keyword on the same line). With both, the
corpus loses no declarations silently (18 files to 0), files with an ERROR go from 72 to 71 with
no new ones, and the trees of the conformance fixture are byte-identical. No published crate
parses both shapes: `tree-sitter-kotlin-sg` 0.4.1 has its own silent losses and would rewrite
nearly all node matching in `src/lang/kotlin.rs`. A git dependency or a `[patch]` does not
reach `cargo install meada`, which builds from the registry.

## Decision

- `vendor/tree-sitter-kotlin-ng/` holds the generated grammar: `src/parser.c` from PR #21's
  head, `src/scanner.c` from PR #13's head, the `tree_sitter/` headers and `LICENSE` from tag
  `v1.1.0`. `PROVENANCE.md` names the commits and the SHA-256 of each file. The files are byte
  copies (`.gitattributes` keeps them `-text`); generated C is never edited by hand.
- The binary's `build.rs` compiles them with `cc`, with the flags of the crate's own build
  script, and `src/lang/kotlin.rs` declares `tree_sitter_kotlin` and wraps it in a
  `tree_sitter_language::LanguageFn`, so the pack calls the grammar as it calls a crate's. The
  published crate carries the sources (about 1.2 MB of the compressed `.crate`), and the
  container build copies them.
- A misparse the fixes miss stays visible: a declaration's hard keyword (`class`, `fun`,
  `interface`, `object`, `typealias`, `val`, `var`) as an operand of an `infix_expression`
  leaves the whole file out (`Facts::unbuilt`), as a file over the qualification budget is. Valid
  Kotlin never names anything with a hard keyword, and the patched grammar produces the shape
  nowhere in the corpus.

## Updating or dropping it

- When upstream releases a version with both fixes, depend on that `tree-sitter-kotlin-ng`
  release, delete `vendor/tree-sitter-kotlin-ng/`, `build.rs` and its `cc` build dependency,
  the `tree-sitter-language` dependency and the extern in `src/lang/kotlin.rs`, and the
  Dockerfile and `.dockerignore` lines. The conformance fixture's trees must stay byte-identical.
- Taking another upstream fix means copying its generated `src/parser.c` or `src/scanner.c`
  from the pull request's head, checking that it differs from the vendored file only by the
  pull request's diff and that the external tokens of `parser.c` and `scanner.c` agree, and
  updating `PROVENANCE.md`. A fix to `grammar.js` alone needs its `parser.c` regenerated
  upstream; Meada does not run the tree-sitter CLI.

## Consequences

- Third-party C enters the build from this repository instead of from the registry, and its
  review is ours: the provenance file and its hashes are the check.
- Two Kotlin grammars cannot coexist in one binary: every Kotlin grammar exports the C symbol
  `tree_sitter_kotlin`, and the linker silently keeps one. Adding a crate that depends on any
  Kotlin grammar (a `tree-sitter-kotlin*` crate, directly or through another dependency) is a
  bug until this vendoring is dropped; `cargo tree -i` names it.
- A C compiler was already required (rusqlite's bundled SQLite and every grammar crate), so
  `cargo install meada` needs nothing new. The repository grows by the 22 MB `parser.c`.
- The remaining visible ERRORs (`when` guards, `suspend {}`, infix `get`, `dynamic`, `$$"..."`,
  `where`, delegation `by D()` with a body) are untouched; open upstream PRs #15, #17, #20, #22
  and #23 are untested.

## Considered options

- `tree-sitter-kotlin-sg` 0.4.1: fewer ERROR files, but it still misparses both shapes, has
  silent losses of its own, and changes the node names the pack reads.
- Staying on 1.1.0 with only the guard: the loss becomes visible, and nothing is recovered.
- A fork published to crates.io: the same sources under a name and a release Meada would have
  to maintain, for a grammar upstream is expected to fix.
