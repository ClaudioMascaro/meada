# Documentation validation for #320

The change moves reader documentation into 23 pages per language and adds 16 versioned
interactive maps. It changes no Rust product code, ADR, Helm chart or console implementation.

## Content and public build

The migration retains all 20 original fenced examples from each README at base
`e8d315c1285352644e37ba4a745de268008e18e3`, byte for byte (60 examples total).
The editorial pass keeps the operational caveats and exact configuration names while
splitting language support, console setup and multi-node deployment into direct references.
The requested no-ai-slop checklist was applied to those complete drafts in pt/en/es.

`mkdocs build --strict` validates every language and builds 48 accessible SVG fallbacks.
`scripts/docs/check.py` checks emitted links and anchors, repository-source links,
translation parity, graph payloads and complete semantic fallback text. Its public-build
negative cases require nonzero exits for missing pages, missing anchors, unknown graphs,
duplicate node IDs, dangling edges, missing translations, nonfinite coordinates, roadmap
cycles and nonexistent graph section references.

The final pre-commit gate also runs the repository's required default Cargo formatting,
Clippy and test commands. The coordinator's handoff records the exact staged tree and durable
receipt after that gate completes. No cloud feature suite is applicable to this documentation
change, and no hosted workflow or deployment was triggered by implementation.

## Browser evidence

A bounded headless Chrome pass exercised all 48 graph instances across 36 localized pages.
It verified expected nodes and edges, group semantics for nodes containing links, card text
fit at 244 by 168 pixels, no document overflow, no console errors, no failed HTTP responses
and no external runtime requests. A scoped CSS override prevents Material's SVG width rule
from collapsing React Flow edge layers.

Representative captures cover desktop and mobile, light and dark, Portuguese, English and
Spanish, architecture, roadmap, admission, console and the reading surface. Keyboard step
selection updates the explanation panel. Delete and arrow keys do not edit graph positions.
Zoom, fit and fullscreen work; wheel gestures continue scrolling the page. The language menu
preserves the current page under `/meada/`, `/meada/en/` and `/meada/es/`. Search returns local
results and the mobile drawer opens without horizontal overflow.

Ordinary installation pages request no React Flow bundle. With JavaScript disabled, or when
the bundle request is blocked, the static diagram and complete open step list remain visible.
Actual code-token contrast on the console reference has minimum ratios of 5.35:1 in light mode
and 6.69:1 in dark mode. The branching graph's initial selected step is fully visible at both
1440-pixel and 390-pixel viewports.

The final packet is under `/tmp/meada-320-react-final/`: `checks.json` contains the measured
results; PNGs capture the stated page or component. Browser QA used a disposable external
Puppeteer installation. A browser and Puppeteer are not documentation build dependencies.
The finish-review verdict and final gate receipt belong in the handoff, avoiding a claim of
approval before those steps complete.
