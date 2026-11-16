# A public corpus for A9's F2: CI steps that install a Python tool and run it

- Date: 2026-11-16
- Question (#854): which public repositories, indexed as one workspace, give at least 30 CI steps
  that install a Python tool of the workspace and at least 30 CI steps that run its console script,
  in forms slice F2 of Spec A9 (#820, ADR 0028) already reads?
- Outcome: one set qualifies. **Set F**, 41 repositories (the five tools `tox`, `nox`, `twine`,
  `build` and `cibuildwheel` with the repositories of `tox-dev` and `scikit-hep` that install or
  run them), gives 64 install steps and 120 program steps by the letter of ADR 0028, 178 of the
  184 across repositories. The program links at `inferred` are 9, under the floor of 30, and 27
  more steps install or run a tool in a shape the gate does not read (section 5). Every other
  candidate was rejected (section 6). The search was run by an agent that does not implement A9.

## 1. The rule, written before the search

Copied from #854, unchanged:

1. A public repository that is a Python tool: a PyPI package with a console script.
2. Public repositories whose GitHub Actions pipelines install that tool in a `run` script in a form
   F2 already reads (`pip install`, `pip3 install`, `python -m pip install`, `uv pip install`,
   `pipx install`, `uv tool install`, `uvx`, `pipx run`) and then run its console script.
3. A set qualifies when its repositories, indexed as one workspace, would give at least 30 install
   steps and at least 30 steps that run the console script, counted from the source.

Two choices this note adds, both about pinning and none about what Meada reads:

- A consumer repository enters a set when the raw text of one of its pipelines or action files
  installs or runs a tool of the set in any shape (an install command or `uvx`/`pipx run` that
  names the tool, or a line that starts with its console script), whether or not the gate of
  ADR 0028 reads that shape. Choosing only the repositories whose steps the gate reads would
  choose code Meada already reads well (ADR 0029, "Open to revision", the floor).
- A repository leaves a set only for its licence (none, or not an SPDX licence the API or the
  LICENSE file at the pin states) or its size (over 100 MB on disk with at most two statements).

## 2. Method

Every repository was read through the GitHub API at one commit, the head of its default branch on
2026-11-16 (`git/trees/<commit>?recursive=1`, then each file through GraphQL at
`<commit>:<path>`): the pipelines (`.github/workflows/*.yml|yaml`), every `action.yml` or
`action.yaml` of the tree, and every `pyproject.toml`. Nothing was cloned, built, installed or run.
Licences are the API's `license.spdx_id`, or the LICENSE file read at the pin where the API says
`NOASSERTION`.

A script of the measurement unit (outside this repository, not part of Meada) listed the
statements from the YAML with the counting rules of section 3, and four of them, drawn at random,
were read back by hand against the raw step. The script was first run on the smoke corpus
(`ClaudioMascaro/meada-smoke-shared`, `-lib`, `-app`) and agreed with every `uses` case of its
`CASES.md`.

## 3. Counting rules

ADR 0028 ("The `run` script and the command table", "Programs of the workspace"), with the readings
`.claude/skills/prove-meada/references/ci.md` records as settled by #831 and #832:

- **Gate.** The shell is the step's `shell`, else `defaults.run.shell` of the CI job, else of the
  pipeline; only `bash`, `sh` or none is read. A script holding a heredoc is not read. Lines joined
  on a trailing `\`; a command is the first word of a line or the one after `&&` or `;`; nothing
  after `||` or a pipe, nor behind `sudo`. A leading `if`, `then`, `do`, `else`, `elif`, `while`
  or `until` is skipped, since a command inside `if` or a loop counts.
- **Words.** Quotes removed; `${{ ... }}` is one unresolved word, never a name. Comments dropped.
- **Install.** A command whose leading words are a row of the table. Each argument that is no
  option, no option's value (by pip's, uv's and pipx's own documentation: `-r`, `-c`, `-e`,
  `--with`, `--python`, `--index-url` and the like), no path, URL or file is a name; extras
  and version specifiers are dropped. Counted once per CI step and package.
- **`uvx <package>`, `pipx run <package>`.** The next word, when it is a literal name and no
  option. When the package declares a console script of that name the step starts it (`exact`);
  otherwise the step is only a consumer of the package (#832), counted with the installs.
- **Program.** A command whose first word is a console script of a `pyproject.toml` of the set
  (`[project.scripts]`, else `[tool.poetry.scripts]`). `exact` when an earlier line or step of
  the same CI job installs its package, else `inferred` (one package declares the script in each
  set below). Counted once per CI step and script.

The levels are ADR 0028's as written. #850 lowers an install by name alone, and the program link
that rests on it (`uvx` and `pipx run` included), to `inferred`: once it lands, every install and
program statement of Set F is `inferred`.

## 4. Set F: Python tools and their CI

The tools, each the only provider of its name and of its console script in the set:

| Package | Console script | Declared at the pin |
| --- | --- | --- |
| `tox` | `tox` | [`tox-dev/tox` pyproject.toml:79](https://github.com/tox-dev/tox/blob/0efe53b8fe12b94e5db218f0538bb4907fe14d9f/pyproject.toml#L79) |
| `nox` | `nox`, `tox-to-nox` | [`wntrblm/nox` pyproject.toml:72](https://github.com/wntrblm/nox/blob/6054d2ad1b0fb1e87f8922eb78abffd0ad8743e1/pyproject.toml#L72) |
| `twine` | `twine` | [`pypa/twine` pyproject.toml:65](https://github.com/pypa/twine/blob/ecad921d4a44091bf01d5b9ec8833d51bfdf58eb/pyproject.toml#L65) |
| `build` | `pyproject-build` | [`pypa/build` pyproject.toml:50](https://github.com/pypa/build/blob/9a03927a4b0afe33ce40dbe5f36bf3bdbc612385/pyproject.toml#L50) |
| `cibuildwheel` | `cibuildwheel` | [`pypa/cibuildwheel` pyproject.toml:55](https://github.com/pypa/cibuildwheel/blob/3a05217802d774b6f176f8d423867b4b068ccd89/pyproject.toml#L55) |

### 4.1 Counts

| Statement | Form | `tox` | `nox` | `twine` | `build` | Total |
| --- | --- | --- | --- | --- | --- | --- |
| install | `uv tool install` | 21 | 8 | 0 | 0 | 29 |
| install | `python -m pip install`, `python3 -m pip install` | 9 | 1 | 1 | 7 | 18 |
| install | `uv pip install` | 0 | 0 | 0 | 1 | 1 |
| install | `pipx run build` (no script named `build`: consumer only) | 0 | 0 | 0 | 16 | 16 |
| program, `exact` | `uvx <tool>`, `pipx run <tool>` | 0 | 35 | 15 | 0 | 50 |
| program, `exact` | console script after an install in the CI job | 53 | 7 | 1 | 0 | 61 |
| program, `inferred` | console script, no install in the CI job | 4 | 5 | 0 | 0 | 9 |

- Install steps: 64 (48 by a row of the install table, 16 by `pipx run`). Program steps: 120 (70
  that run the console script by name, 50 by `uvx` or `pipx run`). By the strictest reading of
  #854 (an install row only, a console script by name only) the set gives 48 and 70.
- 178 of the 184 statements sit in another repository than the tool's: F2 across repositories,
  the case the corpus survey of the release plan found no source for.
- No step installs or runs `cibuildwheel` in a `run` script: its consumers call the action
  `pypa/cibuildwheel@<ref>`, a CI contract to an external action. It stays as a provider.
- No heredoc script of the set mentions a tool, so the heredoc rule hides none.

### 4.2 Repositories

Pin is the commit read; size is the API's `diskUsage`. All 41 total 272 MB.

| Repository | Commit | Licence | MB | Install | Program |
| --- | --- | --- | --- | --- | --- |
| `pypa/build` | `9a03927a4b0afe33ce40dbe5f36bf3bdbc612385` | MIT | 1 | 3 | 8 |
| `pypa/cibuildwheel` | `3a05217802d774b6f176f8d423867b4b068ccd89` | BSD-2-Clause | 8 | 1 | 4 |
| `pypa/twine` | `ecad921d4a44091bf01d5b9ec8833d51bfdf58eb` | Apache-2.0 | 1 | 6 | 1 |
| `scikit-hep/awkward` | `ba8ff569ff54dd2a45a5fa6c32a59431cdb78787` | BSD-3-Clause | 79 | 11 | 21 |
| `scikit-hep/boost-histogram` | `1e5fd6f49c20ef781995c501c2d0932d6d4f5661` | BSD-3-Clause | 3 | 2 | 5 |
| `scikit-hep/cabinetry` | `22a85f7e14759f13126016e05e118c14895fe664` | BSD-3-Clause | 2 | 2 | 1 |
| `scikit-hep/cuda-histogram` | `cfdcae06bba4ade9efe3a388edbbce2d1d72d776` | BSD-3-Clause | 0 | 0 | 1 |
| `scikit-hep/decaylanguage` | `12a11e44bfb96deb77fbb4ba6acd1aba63b96052` | BSD-3-Clause | 2 | 0 | 1 |
| `scikit-hep/fastjet` | `46100f9b29031869b034d46ad246ee72d8a979bb` | BSD-3-Clause | 0 | 1 | 0 |
| `scikit-hep/hepconvert` | `b0a97ebfbe948c370ec36b2822986abecad81092` | BSD-3-Clause | 5 | 1 | 2 |
| `scikit-hep/hepunits` | `40d0f1596373a1a2444c2190ad01fc7253cc655d` | BSD-3-Clause | 0 | 1 | 1 |
| `scikit-hep/hist` | `6706833215dcf0d99037dcd7300362d44a811acb` | BSD-3-Clause | 5 | 3 | 3 |
| `scikit-hep/histoprint` | `0fe396ea602fbc2cff4514e7d74de1c4e6d8a1e8` | MIT | 0 | 1 | 1 |
| `scikit-hep/integration-tests` | `105d4140b7f833c0e8418b69fe6ab371f0a73f67` | BSD-3-Clause | 0 | 0 | 2 |
| `scikit-hep/mplhep_data` | `ca20a268aac04b4e681abbfa511277adf98aa208` | MIT | 8 | 0 | 0 |
| `scikit-hep/numba-stats` | `09e04c3e22d55b3e7ddfd56e3f07123714a133be` | MIT | 0 | 2 | 1 |
| `scikit-hep/particle` | `efe699ccd75e48911bfcc71353198a0d418018a0` | BSD-3-Clause | 2 | 1 | 6 |
| `scikit-hep/pyBumpHunter` | `484c35ce4e2398d06b16941472c6a3d5c1f1b9b4` | BSD-3-Clause | 5 | 1 | 1 |
| `scikit-hep/pyhepmc` | `405e49ea04d3b924254fc3fc70da8aae42e68e94` | BSD-3-Clause | 1 | 1 | 0 |
| `scikit-hep/pyhf` | `7953abc16013759a0e07225a8547ea47ebc468d1` | Apache-2.0 | 60 | 0 | 0 |
| `scikit-hep/pylhe` | `f85fee0a5dd3206620a8fe9aab1b63e068ee6d2f` | Apache-2.0 | 2 | 0 | 1 |
| `scikit-hep/ragged` | `14d45af4b9b086aae736f2ce7426502eaf6bdcfa` | BSD-3-Clause | 0 | 0 | 1 |
| `scikit-hep/resample` | `2774eaaeb30c9de727815c3a62953ad3e7fa94d7` | BSD-3-Clause | 1 | 1 | 0 |
| `scikit-hep/scikit-hep` | `3923b35af75209ed4c0d7508afa319b358d13212` | BSD-3-Clause | 2 | 1 | 1 |
| `scikit-hep/uhi` | `fc1ca144ac9dcd181bdff87543d01a51781fa65e` | BSD-3-Clause | 0 | 1 | 0 |
| `scikit-hep/uproot-browser` | `a7cf4d6f03352ff7a9b0e0660dc221b3ae01c54b` | BSD-3-Clause | 0 | 1 | 1 |
| `scikit-hep/vector` | `955b06fdd5c489a62b46a3020896ce778dc74b4c` | BSD-3-Clause (LICENSE; API `NOASSERTION`) | 2 | 1 | 6 |
| `tox-dev/filelock` | `39bf29042684c32407f7c34bff9f700c8df696e1` | MIT | 3 | 5 | 9 |
| `tox-dev/github-workflows` | `ef0cd88bef592260a48345753b8b509b209ab189` | MIT | 1 | 0 | 2 |
| `tox-dev/pipdeptree` | `55cd28b9208423b0354ddf0e5a4f0923494b6862` | MIT | 2 | 1 | 2 |
| `tox-dev/platformdirs` | `bb5462c86c1d578e7861e91b1ee1803b695805bc` | MIT | 1 | 2 | 4 |
| `tox-dev/release-creator` | `76a6b40e348a21afbe12c757d44c26532a950f87` | MIT | 0 | 2 | 4 |
| `tox-dev/sphinx-autodoc-typehints` | `6a0fd33786be05c0bd7303674c6a510e6c1a56bb` | MIT | 2 | 1 | 2 |
| `tox-dev/toml-fmt` | `55515db5389687c3abae31a7316c431169c14deb` | MIT | 2 | 6 | 19 |
| `tox-dev/tox` | `0efe53b8fe12b94e5db218f0538bb4907fe14d9f` | MIT | 17 | 1 | 2 |
| `tox-dev/tox-extra` | `9e16b11f7a3e9acdddc5b2bb58f5b3d591882055` | MIT | 0 | 1 | 0 |
| `tox-dev/tox-gh` | `b6cb3a192e339b7b7ccfde596a5ec59116d28ce2` | MIT | 0 | 0 | 2 |
| `tox-dev/tox-report` | `344d0f299c4a615db3e7ca33dc7ce421e4fe7a1a` | MIT | 0 | 3 | 2 |
| `tox-dev/turbohtml` | `6c6e490da0eb31fede827a489765c7ca3e3e9f6e` | MIT | 31 | 0 | 0 |
| `tox-dev/workflow` | `d366d5b5d94cf7b72cf858f237593b1c31a5cd7e` (branch `unstable/v1`) | BSD-3-Clause | 0 | 0 | 0 |
| `wntrblm/nox` | `6054d2ad1b0fb1e87f8922eb78abffd0ad8743e1` | Apache-2.0 | 5 | 0 | 3 |

Each repository qualified the same way: it is one of the five tools, or its pipelines install or
run one of them (section 1). `mplhep_data`, `pyhf`, `turbohtml` and `workflow` count zero by the
letter and are in the set for what they hold in shapes the gate does not read (section 5).

## 5. What Set F holds that the gate does not read

27 steps install or run a tool of the set in a shape that gives no side under ADR 0028 as written.
They are the open misses the A9 yardstick has to name per cause, or ceilings to decide:

| Cause | Steps | Where |
| --- | --- | --- |
| `uvx` with an option before the package (`uvx --with tox-uv tox run ...`): the next word is an option, so no package is named | 21 | `turbohtml` 19, `github-workflows` 2 |
| a shell other than `bash`, `sh` or none (`bash -l {0}`, `bash -el {0}`) | 6 | `awkward`: 2 installs of `build`, 4 `pipx run nox` |

Neither needs a new row of the command table: the first is the `uvx` row read past its options,
the second is the gate's shell list. This note proposes neither; both are recorded so the
yardstick counts them before the run.

Also outside the counts: 1 `pipx run --spec ...` and 5 `uvx --from ...` steps that name a package
through an option's value, none of them a tool of the set. The 6 steps of the second cause are the
only tool statements the `bash` variants hide; steps whose shell is not `bash` at all (`python`,
`msys2 {0}`, an expression) were not searched for tools.

## 6. Rejected candidates

| Candidate | Counts from the source | Why rejected |
| --- | --- | --- |
| `tox-dev` alone (37 repositories, `tox-dev/tox` as the tool) | 22 install, 48 program | under 30 install steps; it is part of Set F |
| `scikit-hep` with the five tools, without `tox-dev` | 38 install (20 by an install row), 66 program | 20 by the install table alone; Set F holds it whole |
| `jaraco` (94 repositories install `tox` from one template, `jaraco/skeleton`) | 189 install, 277 program | the API states no licence for 89 of the 94 (`None` or `NOASSERTION`), and the 5 with one give 10 and 10; every statement also comes from one copied file, which the floor of ADR 0029 would count 94 times |
| `pre-commit/pre-commit` as the tool | not counted | its console script is in `setup.cfg`, a manifest the PyPI reader does not read: a new origin |
| `apache/hamilton`, `apache/burr`, `apache/texera` (set E of the release plan) | 0 install, 0 program | they install their own packages by path (`pip install -e .`, `-r requirements.txt`), which gives no side (#831), and by name only packages outside the set |
| `scikit-hep/scikit-hep-testdata` | 1 install, 0 program | 359 MB |
| `scikit-hep/hepstats` | 1 install, 1 program | no licence in the API or at the root |
| `scikit-hep/iminuit` | 2 install, 2 program | its LICENSE puts the vendored Minuit2 code under LGPL-2.1 |
| `tox-dev/tox-pre-commit`, `tox-dev/tox-towncrier` | 0 install, 0 program | LICENSE "modified from the MIT license": no SPDX licence |

## 7. What this corpus cannot show

- That a name alone picks the right package: here the provider is the true publisher of every
  name. The fork probe of #850 covers that.
- The `inferred` level of a program link past the floor: 9 statements.
- The image builder sites of F2: Set F holds none. Across every public set read for the release
  plan there are 12 (`docker/build-push-action` 6 in `apache/devlake`, 3 in `scikit-hep/pyhf`, 2 in
  `apache/texera`; one `docker buildx build --push` in `apache/bifromq`), under the floor.
- `ambiguous` (two packages declaring one script): no two packages of the set share a script name.

## 8. A by-product for the A9 yardstick

The same reading of `tox-dev` (all 37 repositories) counts what slice F1 would link there, since
its repositories call reusable pipelines and actions of `tox-dev/github-workflows`: 194 links to a
pipeline by full name across repositories, 122 to a folder action across repositories, 107 by the
`$/` form, and 56 calls pinned to a commit other than the pin of `github-workflows` (`ref
mismatch`). Report #835's corpus exercised neither the `$/` form nor more than 4 links across
repositories. Pinning and counting it is the A9 yardstick's work, not this ticket's.
