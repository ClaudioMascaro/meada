# Documentation site research for issue 320

Research date: 2026-10-23. Scope: the documentation build, multilingual reading experience,
diagrams and GitHub Pages delivery. This is an engineering research note, outside the published
`docs/site` reader tree. Recommendations below are design judgments informed by the cited
primary sources, not claims that an implementation has passed its acceptance checks.

## Recommended implementation

Use one `mkdocs.yml`, MkDocs 1.6.1, Material 9.7.7 and `mkdocs-static-i18n` 1.3.1. Keep reader
content in `docs/site`, with parallel `pt`, `en` and `es` folders and shared local assets.
Preserve the migrated reader content, visual system, ADR and agent paths. The final site has
23 pages per language (69 total), including roadmap, and 16 focused interactive maps. Make
`mkdocs build --strict` build every language, validate versioned graph data and bundle local
React Flow islands for operational lifecycles, architecture and roadmap. Publish semantic HTML
explanations from the same data alongside each interactive panel. The user's subsequent
direction to use React Flow replaces the earlier Mermaid rendering recommendation; Mermaid
and its browser renderer are no longer recommended dependencies for this implementation.

## Version choices and maintenance

| Dependency | Recommended pin | Evidence and reason |
| --- | --- | --- |
| MkDocs | `1.6.1` | The published stable release includes the 1.6 anchor validation controls. The current 2.0 releases are prereleases. [PyPI release](https://pypi.org/project/mkdocs/1.6.1/) |
| Material | `9.7.7` | The July 2026 release fixes a DOM XSS in search suggestions. Material limits MkDocs to versions below 2. [Changelog](https://squidfunk.github.io/mkdocs-material/changelog/) |
| Static i18n | `1.3.1` | The February 2026 release is available on PyPI and declares Python 3.8 or newer. [PyPI release](https://pypi.org/project/mkdocs-static-i18n/1.3.1/) |
| React Flow | `@xyflow/react@12.12.0` | Published peers accept React and ReactDOM `>=17`; no Node engine restriction is declared. [Official package metadata](https://registry.npmjs.org/@xyflow%2freact/12.12.0) |
| React | `react@19.3.1` | Published Node engine is `>=0.10.0`. [Official package metadata](https://registry.npmjs.org/react/19.3.1) |
| ReactDOM | `react-dom@19.3.1` | Requires React `^19.3.1`; use the same exact version for both packages. [Official package metadata](https://registry.npmjs.org/react-dom/19.3.1) |
| esbuild | `esbuild@0.28.2` | Published Node engine is `>=18`; supplies the small browser bundle build. [Official package metadata](https://registry.npmjs.org/esbuild/0.28.2) |

These four npm versions were verified against the official registry's `latest` metadata on
the research date using read-only requests. Their declared engine and peer requirements
accept Node 22.23.1 and the exact React pair above. This is dependency-metadata compatibility,
not a claim that installation, bundling or browser tests have run. No packages were installed
or builds executed during this research update.

Material is in maintenance mode and upstream support ends on May 5, 2027. This does not change
the requested stack for this issue. It does justify small theme overrides, ordinary Markdown,
portable CSS and versioned graph data, which reduce the work of a future migration. Avoid
the deprecated Material projects and typeset plugins. [Material maintenance notice and
deprecations](https://squidfunk.github.io/mkdocs-material/changelog/)

Pin the complete Python dependency graph, preferably with SHA-256 hashes and
`pip install --require-hashes -r ...`. Hash mode requires every transitive dependency to be
listed and pinned. [pip secure installation](https://pip.pypa.io/en/stable/topics/secure-installs/)
Commit `package-lock.json` for diagram and font packages and use `npm ci`, which rejects
manifest/lock disagreement and does not update either file. [npm ci](https://docs.npmjs.com/cli/v11/commands/npm-ci/)

## Multilingual navigation and search

Material alone assigns one site language per configuration. It supports Brazilian Portuguese,
English and Spanish, and its language selector needs absolute alternate URLs. The i18n
plugin avoids maintaining three independent build commands while keeping a single strict
build as the public seam. [Material language configuration](https://squidfunk.github.io/mkdocs-material/setup/changing-the-language/)

`docs_structure: folder` explicitly supports a folder per language plus shared assets outside
those folders. Keep identical relative page names across the three trees.
[Plugin folder structure](https://ultrabug.github.io/mkdocs-static-i18n/setup/choosing-the-structure/)
Use the Portuguese locale as `default: true`, so it builds at the project root. English and
Spanish build under their language prefixes. Language configuration can override theme
settings, descriptions and other site metadata. If the source folder uses `pt`, ensure the
Portuguese theme and emitted HTML identify Brazilian Portuguese correctly; inspect the actual
`lang` attribute rather than assuming locale normalization.
[Plugin language settings](https://ultrabug.github.io/mkdocs-static-i18n/setup/setting-up-languages/)

The plugin supports `nav_translations` and complete per-language `nav` overrides. Prefer one
page structure and translated labels for this site, so readers retain their location and
translation coverage stays reviewable. Navigation paths remain the logical names such as
`server.md`, without locale prefixes. [Localized navigation](https://ultrabug.github.io/mkdocs-static-i18n/setup/localizing-navigation/)

Keep `reconfigure_material: true`: the plugin adjusts the theme language and contextual
language switcher. Do not enable `navigation.instant`, which its compatibility guide
explicitly rejects with the multilingual switcher.
[Material integration](https://ultrabug.github.io/mkdocs-static-i18n/setup/setting-up-material/)
Keep `reconfigure_search: true`, which removes fallback duplicates from search results.
[Search integration](https://ultrabug.github.io/mkdocs-static-i18n/setup/setting-up-search/)

Set `fallback_to_default: false` and separately compare the three Markdown file sets. The
plugin otherwise defaults to filling missing translations with default-language pages; simply
disabling fallback removes missing pages rather than proving translation completeness.
File-set equality and the README migration inventory are therefore Meada-specific acceptance
checks. [Fallback behavior](https://ultrabug.github.io/mkdocs-static-i18n/setup/controlling-your-builds/)

## Strict links and the project path

Set `site_url: https://claudiomascaro.github.io/meada/`. MkDocs uses its path both for canonical
URLs and the local preview mount. Use relative Markdown links to physical `.md` files.
MkDocs 1.6 defaults missing anchors to informational messages, so strict mode alone is
insufficient. Make these severities explicit:

```yaml
validation:
  nav:
    omitted_files: warn
    not_found: warn
    absolute_links: warn
  links:
    not_found: warn
    anchors: warn
    absolute_links: warn
    unrecognized_links: warn
```

Warnings fail `mkdocs build --strict`. Alternatively, `absolute_links: relative_to_docs`
allows root-relative source links and rewrites them for the deployed site; use one convention
consistently. [MkDocs configuration and validation](https://www.mkdocs.org/user-guide/configuration/)

Implementation acceptance should also inspect emitted HTML `href` and asset `src` values,
including template-generated language switches. Confirm that internal URLs resolve below
`/meada/`, target files exist and fragment IDs exist in the target document. This supplements
Markdown validation for links introduced by templates and diagram transforms. Preview at
`http://127.0.0.1:8000/meada/`, then test a missing page and a missing fragment in temporary
source copies through the exact public build command. Each must exit unsuccessfully.

## React Flow islands for diagrams and roadmap

React explicitly supports multiple `createRoot` calls when only parts of an existing page use
React. Mount one root per diagram container and use a unique `identifierPrefix` for each. Keep
the explanatory HTML outside those mount containers because React takes ownership of their
contents. This fits the existing MkDocs pages without a React application shell.
[React createRoot](https://react.dev/reference/react-dom/client/createRoot)

Bundle React, ReactDOM, React Flow, local graph data and island code into static browser assets
using esbuild. Its official guide supports JSX, self-contained bundles, browser targets and
production minification. One small build script is sufficient; there is no need for a frontend
framework, development server or backend in the deployed site.
[esbuild browser bundling](https://esbuild.github.io/getting-started/#bundling-for-the-browser)
Recommended build options are `bundle: true`, `platform: 'browser'`, `format: 'esm'`,
`jsx: 'automatic'`, an explicit supported browser target, production `NODE_ENV` and minification.
Emit the resulting JavaScript and CSS into the MkDocs asset tree and resolve their URLs through
MkDocs so `/meada/` is preserved. Package installation happens during environment setup;
ordinary site builds use local locked dependencies and make no CDN requests.
[esbuild build API](https://esbuild.github.io/api/)

### Read-only interaction

The following is a recommended starting configuration, to be tuned by the actual visual pass.
React Flow exposes all of these interaction and viewport controls:

```jsx
<ReactFlow
  nodes={nodes}
  edges={edges}
  nodeTypes={nodeTypes}
  nodesDraggable={false}
  nodesConnectable={false}
  edgesReconnectable={false}
  deleteKeyCode={null}
  selectionKeyCode={null}
  multiSelectionKeyCode={null}
  elementsSelectable={false}
  nodesFocusable
  edgesFocusable
  autoPanOnNodeFocus
  panOnDrag
  panOnScroll={false}
  zoomOnScroll={false}
  zoomOnPinch
  zoomOnDoubleClick={false}
  preventScrolling={false}
  fitView
  minZoom={0.4}
  maxZoom={1.6}
  fitViewOptions={{ padding: 0.18, maxZoom: 1 }}
  ariaLabelConfig={localizedAriaLabels}
>
  <Controls showInteractive={false} />
</ReactFlow>
```

`preventScrolling={false}` lets normal document scrolling continue over a panel.
`deleteKeyCode={null}` disables deletion shortcuts. The zoom bounds above are design defaults,
not measured requirements. Individual node or edge overrides must not re-enable editing.
[React Flow component API](https://reactflow.dev/api-reference/react-flow)

The Controls component supplies zoom and fit-view buttons. Set `showInteractive={false}` so
readers cannot toggle the flow into an editable mode. An explicit, localized `aria-label` on
the Controls wrapper can describe the entire control group.
[Controls API](https://reactflow.dev/api-reference/components/controls)
Keep source nodes and edges immutable. Do not add editing callbacks, save controls or local
storage persistence. Any transient React state belongs to presentation, such as the viewport
or an expanded explanation. These are Meada-specific scope decisions.

### Keyboard access and localization

Keep `nodesFocusable`, `edgesFocusable` and `autoPanOnNodeFocus` enabled. Focus can move through
the graph with Tab and the canvas brings the focused node into view. Disabling dragging also
prevents arrow-key movement of nodes. Use `ariaRole: 'group'` on node wrappers that contain
real links or buttons; an outer button role would incorrectly nest interactive semantics.
Visible focus rings must survive the custom CSS.
[React Flow accessibility](https://reactflow.dev/learn/advanced-use/accessibility)

Provide localized `ariaLabel` strings for nodes and edges, describing the label, status and
relationship in context. Nodes support `ariaRole`, `focusable` and `domAttributes` for additional
semantics. Use native `<a>` elements for milestone/spec links inside custom nodes, with a stable
Tab order and a `nopan` class where pointer interaction must not start canvas panning.
[Node API](https://reactflow.dev/api-reference/types/node),
[interaction configuration](https://reactflow.dev/api-reference/react-flow)

Translate these `ariaLabelConfig` keys for `pt-BR`, `en` and `es`:

- `controls.ariaLabel`, `controls.zoomIn.ariaLabel`, `controls.zoomOut.ariaLabel`,
  `controls.fitView.ariaLabel` and `controls.interactive.ariaLabel`.
- `node.a11yDescription.default`, `node.a11yDescription.keyboardDisabled` and
  `edge.a11yDescription.default`.
- `handle.ariaLabel`, and `minimap.ariaLabel` only if a minimap is included.

Replace the default instructions that mention selecting, moving or deleting elements with
accurate reading and navigation instructions for this configuration. The live movement
message is also configurable, but should not be announced by an immutable diagram.
[AriaLabelConfig API](https://reactflow.dev/api-reference/types/aria-label-config)

### Custom visual system and responsive panels

Define reusable `nodeTypes` outside the rendering component. Custom nodes receive their
versioned text and status through `data`; they can render typographic labels, metadata,
status badges and links as ordinary React components.
[Custom nodes](https://reactflow.dev/learn/customization/custom-nodes)
Import `@xyflow/react/dist/base.css` to retain the required structural styles, then apply
Meada's own scoped node, edge, background and control styles. The theming guide explicitly
supports this approach. Reuse local IBM Plex and green `#17685b`; distinguish planned and
delivered states using visible words and border treatment as well as color.
[React Flow theming](https://reactflow.dev/learn/customization/theming)

The official [Turbo Flow example](https://reactflow.dev/examples/styling/turbo-flow) demonstrates
that nodes, edges and the surrounding controls can share a distinct visual system. For Meada,
use a quiet reading panel, compact metadata and clear step hierarchy. Reserve animation for
short user-triggered viewport changes; avoid continuously moving edges. This is an adaptation
to the documentation's existing aesthetic, not a request to import the example's appearance.

React Flow needs a parent with actual width and height. Give each panel `width: 100%` and an
explicit responsive height, with enough space for controls and a caption, rather than relying
on content height. [React Flow container requirements](https://reactflow.dev/learn/troubleshooting/common-errors#the-react-flow-parent-container-needs-a-width-and-a-height-to-render-the-graph)
Fit the graph initially, and refit only after a meaningful container change or the reader's
fit-view action so resize handling does not continually reset a chosen zoom. `fitView` accepts
padding, min/max zoom and duration; use duration zero when reduced motion is requested.
[FitViewOptions](https://reactflow.dev/api-reference/types/fit-view-options)
At small widths, keep text legible through zoom and provide the adjacent linear explanation.
A large roadmap should not be the only way to discover its milestones or spec links.

### Versioned data, fallback and strict validation

Use checked-in graph data with stable graph/node/edge IDs, finite authored coordinates,
explicit node kinds, shared structural relationships and complete localized labels. For these
five curated subjects, authored layouts avoid a separate layout dependency and make changes
reviewable. Treat the roadmap as a fifth graph, with explicit milestone status and spec URLs;
preserve the existing distinction between delivered and planned work. The browser receives
the chosen locale's data from the generated page or bundle and needs no runtime API.

Generate a semantic HTML explanation from that same graph data during the MkDocs build: title,
summary, ordered nodes or milestones, status, dependencies and links. Keep it available even
after enhancement, for example as a visible summary plus expandable detailed list. It must
remain usable when JavaScript is disabled, the bundle fails to load or initialization throws;
a `noscript` block alone only covers the first case. W3C guidance supports an accessible short
identification plus a longer explanation for complex relationships.
[W3C complex images](https://www.w3.org/WAI/tutorials/images/complex/)

React Flow also supports static rendering in v12, but it requires explicit node dimensions,
handle positions and container dimensions for fitting. A full static React Flow rendering is
an optional later improvement; a semantic HTML fallback is sufficient for this implementation
and avoids adding a second layout path.
[React Flow static rendering](https://reactflow.dev/learn/advanced-use/ssr-ssg-configuration)

Keep graph validation and local bundling on the normal `mkdocs build --strict` path. The
`on_pre_build` event is intended for build preparation, `on_files` can register generated
assets and a `PluginError` gives an actionable build failure.
[MkDocs plugin events and errors](https://www.mkdocs.org/dev-guide/plugins/)
The validator should reject duplicate IDs, unknown edge endpoints or node kinds, non-finite
coordinates, missing locale text/status, invalid internal links, unsafe URL schemes and
missing graph references. A roadmap dependency cycle should fail; diagram cycles may be
legitimate and should only be prohibited by that graph's declared contract.

Run failure checks through the public build seam with a dangling endpoint, duplicate node ID
and missing translation. Require nonzero exit for each. Verify generated fallback links and
asset URLs under `/meada/`, then inspect the five subjects in every language for keyboard
focus, pan/zoom, mobile scrolling, local fonts, reduced motion and truthful roadmap status.
Compare two builds under the same lockfile and toolchain before asserting byte reproducibility.

### License notices and attribution

React Flow's core license is MIT and requires its copyright and permission notice to accompany
distributed copies or substantial portions of the software.
[React Flow MIT license](https://github.com/xyflow/xyflow/blob/main/LICENSE)
Keep the default visible React Flow attribution. The maintainer's current attribution page
asks users to subscribe to Pro when removing it; retaining it avoids that choice and supports
the upstream project. That request is separate from retaining the MIT notices in distributed
assets. [Upstream attribution guidance](https://reactflow.dev/remove-attribution)

Ship the relevant third-party license texts with the generated site, including the bundled
React/ReactDOM and React Flow dependency notices. Preserve legal comments with esbuild's
`legalComments: 'eof'` or `'linked'`; the latter emits a companion legal-comments file that must
also be deployed. A generated license inventory can accompany this, but retaining comments
alone should not be assumed to collect every dependency's complete license text.
[esbuild legal comments](https://esbuild.github.io/api/#legal-comments)

## Fonts and reading experience

Set `theme.font: false` to stop Material from loading Google Fonts. Add local `@font-face`
declarations through a custom stylesheet.
[Material font configuration](https://squidfunk.github.io/mkdocs-material/setup/changing-the-fonts/)
Fontsource packages provide self-hostable assets; copy the selected WOFF2 files from exact
package versions into the documentation asset tree, retaining the package license. This is an
asset preparation step; the small esbuild pipeline above exists for the React islands.
[Fontsource installation](https://fontsource.org/docs/getting-started/install)
IBM Plex Sans and Mono fit the technical reading direction. IBM publishes Plex under the
SIL Open Font License. Preserve its license alongside the distributed fonts.
[IBM Plex license](https://github.com/IBM/plex/blob/master/LICENSE.txt)

The following first-party references inform the proposed information architecture. The
observations come from the pages' readable content and navigation; they do not constitute
pixel-level or mobile conformance audits.

| Reference | Observed pattern | Application to Meada |
| --- | --- | --- |
| [Linear Docs](https://linear.app/docs) | Search and persistent topic groups accompany a brief introduction and a small set of useful starting pages. | Use a restrained documentation home with clear next actions and a steady navigation rail. |
| [Stripe documentation](https://docs.stripe.com/) | Guides and examples are organized around concrete integration tasks and quickstarts. | Lead with installation and the first useful query, then link into operational guides and reference material. |
| [Cloudflare developer documentation](https://developers.cloudflare.com/) | Search, a filterable sidebar, learning paths, use cases and reference architecture provide different entrances into a large technical system. | Keep server/cloud/identity concepts discoverable without crowding the initial local-use path. |
| [Astro documentation](https://docs.astro.build/en/getting-started/) | Tutorial, guide and reference are separate navigation choices, with language and theme controls visible at the top. | Preserve task-oriented learning and precise reference pages, with language selection in the persistent header. |

The resulting Meada design recommendation is an editorial reading surface: quiet backgrounds,
strong heading hierarchy, measured line length, the console's green accent, highly legible code,
and diagrams that share the typography and visual tokens. Keep large illustrations and cards
for orientation pages where they help selection. On technical pages, spend the space on prose,
code, tables and explanatory figures. These are design choices for this project, not upstream
framework requirements.

## GitHub Pages workflow

GitHub supports custom build workflows with `configure-pages`, `upload-pages-artifact` and
`deploy-pages`. The deploy job needs `pages: write` and `id-token: write`, a dependency on the
build job, and the `github-pages` environment. The uploaded Pages artifact must contain the
site output without symbolic or hard links.
[GitHub Pages custom workflows](https://docs.github.com/en/pages/getting-started-with-github-pages/using-custom-workflows-with-github-pages)

Use read-only build permissions for pull requests. Allow the deployment job only from the
approved main-branch workflow or an explicit dispatch with the same branch condition. Keep
the Pages write permissions on that job, and retain environment protection. Repository Pages
configuration and the actual deployment remain coordinator actions.

Pin actions to full commit SHAs verified in each upstream repository, with release comments
for maintainability. GitHub recommends full SHAs because tags may move.
[GitHub secure action references](https://docs.github.com/en/actions/reference/security/secure-use)
The following upstream release commits were verified during this research:

| Action | Release | Full commit |
| --- | --- | --- |
| `actions/setup-python` | `v7.1.0` | [`6d4fd22b20465ccc0f7354476588e8df7c9ca0e0`](https://github.com/actions/setup-python/commit/6d4fd22b20465ccc0f7354476588e8df7c9ca0e0) |
| `actions/configure-pages` | `v6.0.0` | [`45bfe0192ca1faeb007ade9deae92b16b8254a0d`](https://github.com/actions/configure-pages/commit/45bfe0192ca1faeb007ade9deae92b16b8254a0d) |
| `actions/upload-pages-artifact` | `v5.0.0` | [`fc324d3547104276b827a68afc52ff2a11cc49c9`](https://github.com/actions/upload-pages-artifact/commit/fc324d3547104276b827a68afc52ff2a11cc49c9) |
| `actions/deploy-pages` | `v5.0.1` | [`368f82528645a54fb793d4d04e342629a3f51346`](https://github.com/actions/deploy-pages/commit/368f82528645a54fb793d4d04e342629a3f51346) |

The local acceptance boundary remains the strict build, its deliberate failure checks, and
desktop/mobile inspection of representative Portuguese, English and Spanish pages and all
four diagram subjects and the roadmap. Hosted workflow success must only be reported after
an actual run.
