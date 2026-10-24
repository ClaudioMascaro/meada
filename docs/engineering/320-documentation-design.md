# Documentation surface contract

Issue #320 creates a Read surface for developers finding code relationships and operators
running shared deployments. Product truth is in PRODUCT.md and the preserved README material.
The explicit founder direction supersedes a new design interview. MkDocs Material supplies
navigation, search and accessibility primitives; this is a custom editorial treatment of them.

## Visual direction

DESIGN_VARIANCE 6, MOTION_INTENSITY 3, VISUAL_DENSITY 5. The incumbent console's green
`#17685b`, restrained borders and neutral reading surfaces inform the documentation, without
changing the console. A compact dark green masthead anchors a warm, light reading surface;
a complete dark palette follows system preference with a manual switch. Self-hosted IBM Plex
Sans and Mono bring a technical publication voice with full Portuguese and Spanish glyphs.

The first viewport answers what Meada does and gives real install/query commands. The index
routes readers by task, while every reference retains a persistent navigation rail and a
local table of contents. Content measure targets 72 characters; large examples scroll inside
code blocks. Search, language switching and navigation use Material's maintained behavior.
Instant navigation is deliberately disabled because it conflicts with static-i18n switching.

Sixteen focused maps explain operational lifecycles, admission, versions, ownership, provider
responsibilities, planned milestones and the requested StatefulSet/S3 reference deployment.
Each uses custom React Flow cards, restrained connection lines and deterministic layouts.
Planned work has explicit status and specification links. Roadmap branches follow tracked
prerequisites without inventing dates or a dependency between S3 and S4.

Selecting a step reveals its explanation and a local reference link. A native selector and
previous/next buttons provide keyboard access. Pan, zoom, fit, minimap and fullscreen support
inspection without editing the diagram or trapping page scrolling. Cards retain a readable
initial scale on mobile; fitting a whole graph is an explicit reader choice.

Each map has an accessible static SVG and complete semantic step list from the same data.
They remain available without JavaScript. Ordinary reading pages do not load React Flow.
The local bundle includes dependency notices; IBM Plex fonts are self-hosted. No CDN,
external runtime API, tracking or invented product feature is involved.

Long references are organized by task, with commands visible and secondary protocol details
in named disclosures. The requested no-ai-slop editorial pass splits the former capabilities
wall into language explanations and comparison tables, defines job/pod/generation before the
S3 architecture, and rewrites console setup with direct instructions in all three languages.

## Grounded alternatives and chosen authority

The audience's familiar systems considered were technical handbooks, API references,
architecture atlases, Unix manuals, lab notebooks, engineering schematics and field guides.
The skill's concept seed ran (a0811cc0, assigned 7), with no remote challengers available.
The user's pinned editorial documentation direction takes precedence over that assignment.
The selected world combines the structure of a technical handbook with the legibility of
engineering schematics, not a marketing landing page. No decision-board or generated imagery
is needed for this explicitly settled, text-and-diagram surface.

## Boundaries and checks

Existing console UI, ADRs and product runtime are unchanged. S1/S2 are delivered; S3/S4/S5
are explicitly planned and linked to their specs. Complete pt/en/es file parity is a build
requirement. Strict links, anchors and diagram failure cases are checked through the public
build command. Desktop/mobile, light/dark, keyboard focus, search, language switching and
project-path assets are checked in one batched pass, then one confirmation at most.
