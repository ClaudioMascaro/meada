---
name: Meada Documentation
description: A technical handbook with readable engineering maps.
colors:
  primary: '#215b49'
  accent: '#17685b'
  link: '#176252'
  paper: '#fafbf8'
  ink: '#263d34'
  muted: '#56675e'
  rule: '#d7e0d7'
  panel: '#f0f4ee'
  code-paper: '#eff3ee'
  code-ink: '#254438'
  dark-paper: '#15231d'
  dark-ink: '#e4ece3'
  dark-muted: '#b1c1b4'
  dark-rule: '#385044'
  dark-panel: '#20342a'
  dark-link: '#9ad1b4'
  dark-code-ink: '#d6e7d9'
  flow-node: '#ffffff'
  flow-muted: '#53695b'
  flow-rule: '#c9d8cd'
  dark-flow-paper: '#192b22'
  dark-flow-node: '#253e30'
  dark-flow-muted: '#becdbf'
  dark-flow-rule: '#4b6955'
  dark-flow-control: '#304b39'
  connection: '#587b68'
  planned: '#927138'
  guardrail: '#a35a43'
typography:
  display:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 2.1rem
    fontWeight: 500
    lineHeight: 1.13
    letterSpacing: -0.035em
  headline:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 1.32rem
    fontWeight: 500
    lineHeight: 1.3
    letterSpacing: -0.02em
  title:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 1.05rem
    fontWeight: 600
  body:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 0.84rem
    fontWeight: 400
    lineHeight: 1.75
  lead:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 1.04rem
    fontWeight: 400
    lineHeight: 1.65
  navigation:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 0.68rem
    fontWeight: 400
    lineHeight: 1.4
  code:
    fontFamily: IBM Plex Mono, SFMono-Regular, Consolas, Menlo, monospace
    fontSize: 0.68rem
    fontWeight: 400
    lineHeight: 1.75
  control:
    fontFamily: IBM Plex Sans, sans-serif
    fontSize: 12px
    fontWeight: 500
    lineHeight: 1
  map-title:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 18px
    fontWeight: 600
    lineHeight: 1.25
    letterSpacing: -0.015em
  map-body:
    fontFamily: IBM Plex Sans, -apple-system, BlinkMacSystemFont, Helvetica, Arial, sans-serif
    fontSize: 15px
    fontWeight: 400
    lineHeight: 1.45
rounded:
  inline: 3px
  tag: 4px
  control: 6px
  panel: 7px
  map: 12px
spacing:
  inset: 1rem
  content-gap: 2rem
  flow-gap: 0.4rem
  flow-title-gap: 9px
components:
  button-map:
    backgroundColor: '{colors.panel}'
    textColor: '{colors.ink}'
    typography: '{typography.control}'
    rounded: '{rounded.control}'
    padding: 0 9px
    height: 36px
  button-map-hover:
    backgroundColor: '{colors.flow-node}'
  button-step:
    textColor: '{colors.link}'
    padding: 3px 0
  search:
    rounded: '{rounded.control}'
    height: 1.8rem
  step-selector:
    backgroundColor: '{colors.panel}'
    textColor: '{colors.ink}'
    typography: '{typography.control}'
    rounded: '{rounded.control}'
    padding: 0 22px 0 8px
    height: 36px
  navigation:
    textColor: '{colors.ink}'
    typography: '{typography.navigation}'
  navigation-active:
    textColor: '{colors.link}'
  stage:
    backgroundColor: '{colors.panel}'
    textColor: '{colors.ink}'
    rounded: '{rounded.tag}'
    padding: 4px 5px
  reading-path:
    textColor: '{colors.link}'
    padding: 1rem 0 1.25rem
  code-block:
    backgroundColor: '{colors.code-paper}'
    textColor: '{colors.code-ink}'
    typography: '{typography.code}'
    rounded: '{rounded.panel}'
    padding: 1.2rem
  callout:
    backgroundColor: '{colors.panel}'
    rounded: '{rounded.panel}'
  map-step:
    backgroundColor: '{colors.flow-node}'
    textColor: '{colors.ink}'
    rounded: '{rounded.map}'
    padding: 15px 16px 12px
    width: 244px
    height: 168px
  map-step-active:
    backgroundColor: '{colors.panel}'
---

# Design System: Meada Documentation

## Overview

**Creative North Star: "The Technical Handbook"**

The technical handbook gives long explanations a calm, deliberate reading surface. Forest green anchors the masthead, pale green neutrals separate supporting material, and IBM Plex connects prose, commands and engineering maps with a consistent technical voice. Density comes from useful navigation and structured information, while the article remains the visual center.

This system applies to the documentation in `docs/site`, including the read-only maps built from `scripts/docs`. The product console has its own implementation and is outside this document. The visual direction is settled in `docs/engineering/320-documentation-design.md`; the normative values here come from the shipped styles and MkDocs configuration.

**Key Characteristics:**

- Long-form reading with a bounded measure and clear heading hierarchy.
- Forest-green light and dark themes with thin structural rules.
- Local IBM Plex Sans and Mono across Portuguese, English and Spanish.
- Read-only engineering maps with explicit states and equivalent text.

## Colors

Forest greens define the identity; subtly green paper, ink and rules keep both themes coherent.
The frontmatter contains the normative colors. The sidecar's synthesized tonal strips are
panel previews, not additional implementation tokens.

### Primary

- **Forest masthead** (`primary`): the light-theme header anchors navigation without competing with the article.
- **Interactive green** (`accent`, `link`): focus outlines and links use related, distinct greens. Dark-theme interactions use `dark-link`.

### Secondary

- **Planned ochre** (`planned`): dashed planned relationships and node boundaries.
- **Guardrail clay** (`guardrail`): failure relationships and guardrail node boundaries.
- **Connection green** (`connection`): ordinary map edges.

### Neutral

- **Reading paper and ink** (`paper`, `ink`, `muted`): the article, heading and supporting text roles.
- **Panel and rule** (`panel`, `rule`): callouts, tool summaries, table headers and fine separators.
- **Code paper and ink** (`code-paper`, `code-ink`): commands and examples, with lexical syntax colors maintained in the stylesheet.
- **Map paper, node and rule** (`paper`, `flow-node`, `flow-muted`, `flow-rule`): distinct map layers with the same green cast as the article.
- **Dark reading roles** (`dark-paper`, `dark-ink`, `dark-muted`, `dark-rule`, `dark-panel`, `dark-code-ink`): the full reading surface follows the selected theme.
- **Dark map roles** (`dark-flow-paper`, `dark-flow-node`, `dark-flow-muted`, `dark-flow-rule`, `dark-flow-control`): map layers remain distinct in dark mode.

**The State Has Words Rule.** Planned and failure paths carry labels and distinct line treatments as well as color. Do not make hue the only explanation of a relationship.

## Typography

**Display and Body Font:** IBM Plex Sans, with the installed platform sans-serif stack as fallback.
**Code Font:** IBM Plex Mono, with SFMono-Regular, Consolas and Menlo as fallbacks.

The pairing is technical and readable without a separate ornamental display face. Sans weights
400, 500, 600 and 700 and the Mono 400 face are self-hosted. Font synthesis is disabled.

### Hierarchy

- **Display:** page titles use the frontmatter `display` role, medium weight with tight tracking and balanced wrapping.
- **Headline and title:** second-level headings use `headline`; third-level headings use the smaller, heavier `title` role.
- **Body and lead:** `body` supports long paragraphs; `lead` provides a larger, muted introduction.
- **Navigation:** `navigation` is compact, with active links gaining weight and the link color. Uppercase section labels organize the navigation rail, not the article's heading hierarchy.
- **Code:** `code` is the block-example role. Inline code is relative to its surrounding text (0.77em).
- **Map title and body:** pixel-sized roles preserve a stable card composition while the canvas zooms. Control labels use the separate `control` role.

The CSS uses Material's root sizing: 125% normally, 137.5% above 100em and 150% above
125em. Preserve the recorded rem units rather than assuming a 16px root. Below 40em,
body text is 0.82rem, page titles are 1.8rem and second-level headings are 1.22rem.

**The Reading Measure Rule.** Keep paragraphs and ordinary lists within the established article measure. Let code and maps manage their own overflow without widening the page.

## Layout

The centered shell has a maximum width of 76rem. The primary rail is 12.8rem on wide
screens; the local contents rail is 11.5rem. Article content is capped at 48rem, with
paragraphs and ordinary lists capped at 72ch. The desktop article has horizontal margins
of 2.1rem; these reduce to 1.25rem below 60em and 0.95rem below 40em. Above 76.25em,
page titles also have a 20ch measure. Material owns the rail-to-drawer behavior.

Whitespace follows text and task boundaries, not a rigid invented spacing scale. The small
set of spacing tokens records recurring values. The article starts 2rem below the header;
second-level sections use a larger 2.7rem top separation. Reading paths form two columns,
and the three-tool summary forms three columns. Both collapse to one column below 40em.

Maps remain contained within the article. Their canvas is 460px tall, or 430px below 40em.
Cards retain their 244px by 168px geometry as the viewport changes. The mobile selector
occupies its own toolbar row; controls increase from 36px to 38px tall. Fit-to-view is an
explicit reader action. Fullscreen removes the shell radius and lets the canvas fill the
available space, with the detail inspector bounded to 30vh.

## Elevation & Depth

The custom header, navigation title, callouts and disclosures explicitly remove shadows.
Panel tones, node surfaces and thin borders create depth in normal reading. Material retains
its temporary search and navigation overlays; their behavior is inherited, not a new family
of decorative shadows. The opened search form uses a soft shadow (0 0 .6rem #00000012).

**The Quiet Surface Rule.** Article containers stay flat. Use a thin rule and a tonal surface to establish grouping; reserve inherited overlay elevation for temporary navigation and search.

## Shapes

Small radii distinguish inline code, stage tags and controls. Reading containers share the
`panel` radius, while map shells and map nodes share the larger `map` radius. Reading paths
remain open rows separated by a rule. Table borders and search controls use `control`.
The map's rounded rectangles and smooth step edges express a directed process; dashed
borders identify planned work. SVG icons use outlined geometry rather than text glyphs.

## Components

### Buttons

Map controls are compact bordered buttons using the `button-map` tokens. Hover changes their
surface to the node color; disabled controls reduce opacity. Zoom and fullscreen controls
use inline SVG with accessible names. Previous and next controls use the lighter `button-step`
treatment. All controls inherit a visible accent outline (3px, offset 4px); focused map nodes
use a 5px outline offset. These are reading controls, not a product action hierarchy.

### Inputs / Fields

Search retains Material's maintained search behavior and responsive overlay. Its custom
font size is 0.74rem and its resting form uses the `control` radius. The desktop resting field
is translucent within the masthead; opening search changes it to the reading surface.
The map's native step selector shares the toolbar button surface, border and typography,
providing a keyboard-accessible route to each step.

### Navigation

The primary rail groups references by task; the secondary rail tracks the current article.
Thin rules separate each rail from adjacent content. Active links gain medium emphasis through
weight and link color; hover uses the accent color. The persistent header holds search,
language and theme switching. Theme defaults follow system preference with manual switching.

### Chips

The map stage tag is a small monospace identifier on the control surface with the `tag` radius.
It is metadata, not a filter. Delivered, planned and failure states also have explicit words
and SVG symbols.

### Cards / Containers

Reading paths are linked rows with a short muted description. Code blocks use a tinted surface,
a thin border, the `panel` radius and the recorded block padding; long lines scroll inside the
block. Callouts use the same radius and a ruled title area. Tables use a tinted header.
Reference and map-description disclosures are flat bordered containers with descriptive
summaries and native open/close behavior.

### Engineering Maps

Maps pair a title and toolbar with a bounded canvas, minimap, interaction instructions and
an explanation below the current step. The active node changes to the control surface and
accent border. Selecting its title, choosing from the selector or using previous/next selects
a step and centers it. Canvas movement uses a short 240ms transition, reduced to zero when
reduced motion is requested. Edges are never animated.

The graph is read-only. It permits inspection through drag, zoom, fit and fullscreen while
ordinary wheel scrolling continues to move the page. The same underlying content supplies
a static SVG and complete semantic step list when JavaScript is unavailable. React Flow
loads only on pages with maps. Preserve this relationship when adding a map.

## Do's and Don'ts

### Do:

- **Do** reuse the local IBM Plex fonts and preserve complete Portuguese, English and Spanish interfaces.
- **Do** keep article links visibly underlined and all controls reachable with a visible keyboard focus.
- **Do** use the dark palette for every custom reading and map surface.
- **Do** pair each interactive map with its static SVG, semantic steps and local references.
- **Do** respect reduced motion and keep ordinary page scrolling available over maps.

### Don't:

- **Don't** introduce marketing hero treatments or decorative imagery into this reading system.
- **Don't** use the map card shape to turn ordinary prose and navigation into a wall of cards.
- **Don't** encode planned or failure states through color alone.
- **Don't** replace local assets with remote font or diagram runtime dependencies.
