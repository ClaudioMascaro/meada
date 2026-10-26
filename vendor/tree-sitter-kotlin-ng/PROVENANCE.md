# Provenance

The Kotlin grammar Meada compiles (`build.rs`, ADR 0017): the published tree-sitter-kotlin-ng
1.1.0 with two upstream pull requests not merged at the time of vendoring, and one local patch
(below). Every file but `src/scanner.c` is a byte copy of an upstream file; `src/scanner.c` is
PR #13's head plus the patch, which is kept as a file and applied, never edited in place.

- Upstream: https://github.com/tree-sitter-grammars/tree-sitter-kotlin (MIT, `LICENSE`).
- Base: tag `v1.1.0`, commit `77dd60ea0a9003ce062c9728a513ffe1aaff8c82`, the source of the
  `tree-sitter-kotlin-ng` 1.1.0 crate (its `src/` files are identical).
- PR #21, head `590b45a52bcff0df53173838dc621e0e3e540fe6`: `prec.dynamic(-10)` on
  `annotated_expression`, so a declaration under annotations is a declaration. Its `src/parser.c`
  differs from 1.1.0 in two lines (the dynamic precedence of two `annotated_expression` reduces).
- PR #13, head `123d888901879e8cccab82bc1ee341d74c0989bc`, which carries PR #11 (commit
  `dbefdc7727b0fa8981749d519e725f7033f2d4c9`): the external scanner ends a class member before
  `}` or a declaration keyword on the same line. Its `src/scanner.c` differs from 1.1.0 by the
  PR's diff (+37, -1). Neither PR changes the external tokens, so the two files agree.

| File | Taken from | SHA-256 |
| --- | --- | --- |
| `src/parser.c` | PR #21 head | `bdd6e68b193a4d92a5ba4350a0d896aed408f63041467edc87ccf6ff5f135f6d` |
| `src/scanner.c` | PR #13 head, plus `patches/0001-call-after-local-property.diff` | `92a1f00f1b57e4af4ab0e89d233bbb12ff4c9fc88df9db97d7b8da7b8081e990` |
| `src/tree_sitter/alloc.h` | `v1.1.0` | `b29c1c9fb7cc82f58c84b376df1297d6e2737a1d655fd356db0859e3c29c2fea` |
| `src/tree_sitter/array.h` | `v1.1.0` | `5bdf6ed1a78e3409fd443e085ca967a64c188a5d082aaf7f819bccd53a471c94` |
| `src/tree_sitter/parser.h` | `v1.1.0` | `a1f6ef161fbaf48a0e10fca90ef5290a062462b307b3898aa562993853b9f80a` |
| `patches/0001-call-after-local-property.diff` | local, against PR #13's `src/scanner.c` | `6401efc8eb73da6ae50f818032236a48271ce1c661d4c843f4cf1339f567a96e` |
| `LICENSE` | `v1.1.0` | `0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727` |

## Local patches

Each is a unified diff in `patches/`, made against the upstream file named in the table
(`patch -p1` from the `vendor/tree-sitter-kotlin-ng` directory reproduces the vendored file), and
is to be offered upstream. A patch is dropped when upstream ships it.

- `0001-call-after-local-property.diff` (`src/scanner.c`, +54 -12 lines, no new external token, no
  `parser.c` change): after a local `val`, a line starting with `get` or `set` was read as the
  property's accessor, so `val x = Service()` then `get("/x") { }` (a Ktor route), `get { }` or
  `set { }` made an ERROR from the enclosing declaration to the end of the file: the declarations
  after it vanished, and a false symbol could appear. kotlinc reads accessors on members and
  top-level properties only. The scanner now reads `get` at a line start as a getter only when it
  stands alone or is `get()`, and `set` only when it stands alone or is followed by `(` and a
  name or annotation (`set(value)`, `set(@Ann v)`; a name starts with a letter of any script,
  `_` or a backtick), with or without a space before the parenthesis (`set ("k") {` is a call);
  anything else inserts the statement separator. The accessor forms of `src/lang/kotlin.rs`'s `accessors_stay_accessors_beside_the_scanner_patch` parse as before.
  Measured on the 1,691-file corpus of ADR 0017: files with an ERROR 71 to 69, the only
  declaration change a false `P /list` removed, and the trees of the other fixtures unchanged.
  Offered upstream as pull request tree-sitter-grammars/tree-sitter-kotlin#24, which fixes
  issue tree-sitter-grammars/tree-sitter-kotlin#25: when either merges (that pull request, or
  another fix that closes the issue), re-vendor `src/scanner.c` from upstream and drop the patch
  and its row. The infix `get` shape of upstream PR #22 is a separate, unpatched defect.

To check: `sha256sum -c` over the table, and for each upstream copy
`git diff v1.1.0 <head> -- <path>` in a clone of the upstream repository; for `src/scanner.c`, that
the PR #13 head with the patch applied equals the vendored file.
