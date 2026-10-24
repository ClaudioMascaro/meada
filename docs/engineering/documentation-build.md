# Documentation build and publication

The reader site lives in `docs/site/{pt,en,es}`. `docs/adr` and `docs/agents` remain source
references at their existing paths. This build guide is outside the published site's
`docs_dir` and navigation.

## Install and build

Use Python 3.11.15 and Node 22.23.1 (also pinned by `ui/.nvmrc`). Python dependencies and their
hashes are locked in `scripts/docs/requirements.txt`; npm dependencies are locked in
`scripts/docs/package-lock.json`.

```sh
python3 -m venv .venv-docs
. .venv-docs/bin/activate
python -m pip install --require-hashes -r scripts/docs/requirements.txt
npm ci --prefix scripts/docs --no-audit --no-fund
bounded mkdocs build --strict
bounded python scripts/docs/check.py
mkdocs serve --dev-addr 127.0.0.1:8000
```

The build needs no browser, CDN or backend. esbuild produces a local React Flow bundle from
`scripts/docs/flow.jsx` and `flow.css`, retaining dependency license notices. Only pages with
a graph load it; ordinary reading pages do not pay for React. The bundle uses pinned React,
React DOM and `@xyflow/react` packages isolated from the product's UI dependencies.

`docs/site/assets/graphs.json` is the versioned graph source. Each graph has a deterministic
layout, node/edge IDs, pt/en/es titles, summaries, concise node cards, full step explanations
and local section references. A `<!-- meada-graph:key -->` marker places it on one page in
each language. The public strict build checks translation parity, references, coordinates,
edge endpoints, IDs and roadmap cycles before bundling. Missing page or section links also
fail the build.

The hook emits a localized SVG and a complete semantic step list for every graph. Both work
with JavaScript disabled or a failed bundle request. Selecting a card, using the step selector
or pressing the previous/next controls updates a shared explanation panel. Commands remain
in ordinary Markdown reference sections. These are read-only maps, with no editing, saving,
backend or client router.

Generated SVGs and bundles are ignored output under `docs/site/assets/diagrams` and
`assets/interactive`. A hash of bundle sources and the npm lockfile invalidates the local
bundle cache. Delete those output directories for a clean regeneration. `scripts/docs/check.py`
checks emitted URLs, anchors, accessible SVGs, complete text fallbacks and repository-source
links. It then proves that nine broken inputs fail through `mkdocs build --strict`, using
temporary copies without modifying author files.

The hook preserves static contextual language-menu links and removes Material's duplicate
alternate-link enhancement: that enhancement assumes language-root URLs and otherwise asks
for nonexistent per-page sitemaps. Canonical URLs and the generated sitemap remain. The
repository link is static too, avoiding a runtime GitHub release-statistics request.

Add translated pages under identical names in each locale. Missing translations fail the build;
there is no fallback to Portuguese. Relative Markdown links remain inside the current language.
The language switcher retains the current page. Portuguese is at `/meada/`, English at
`/meada/en/` and Spanish at `/meada/es/`.

## GitHub Pages

`.github/workflows/pages.yml` builds and checks pull requests and main, then uploads the `site`
artifact. It pins every action to a commit. Build permissions are read-only. Deployment runs
only through an explicit workflow dispatch with `deploy: true` on main, after its successful
build, with `pages: write` and `id-token: write` scoped to that job and the `github-pages`
environment. The maintainer must first choose **GitHub Actions** in repository Pages settings
and configure the environment's allowed branch. This change does not enable Pages or trigger
a deployment. The configured canonical URL is `https://claudiomascaro.github.io/meada/`.

For a static preview that also tests the project prefix, serve the parent of a directory named
`meada` containing the generated `site` files, then visit `/meada/`. `mkdocs serve` is the
ordinary editing preview. Run the repository's mandatory default Cargo gates before committing.
