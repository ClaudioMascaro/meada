# Provenance

The Kotlin grammar Meada compiles (`build.rs`, ADR 0017): the published tree-sitter-kotlin-ng
1.1.0 with two upstream pull requests not merged at the time of vendoring. Every file is a byte
copy of an upstream file; none is edited by hand.

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
| `src/scanner.c` | PR #13 head | `c739ec29bffff4a445963d461c906634d418b828998de6a8df3c0e5d438703a9` |
| `src/tree_sitter/alloc.h` | `v1.1.0` | `b29c1c9fb7cc82f58c84b376df1297d6e2737a1d655fd356db0859e3c29c2fea` |
| `src/tree_sitter/array.h` | `v1.1.0` | `5bdf6ed1a78e3409fd443e085ca967a64c188a5d082aaf7f819bccd53a471c94` |
| `src/tree_sitter/parser.h` | `v1.1.0` | `a1f6ef161fbaf48a0e10fca90ef5290a062462b307b3898aa562993853b9f80a` |
| `LICENSE` | `v1.1.0` | `0eea8dc45e89deeb03c7799bbbc7b4688f365fb274562f4540ecfebdea82e727` |

To check: `sha256sum -c` over the table, and for each file
`git diff v1.1.0 <head> -- <path>` in a clone of the upstream repository.
