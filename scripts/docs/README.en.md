# Site build

Generation and validation of the multilingual Meada site.

## Interface

MkDocs hooks validate graphs, generate SVGs and expand page markers. The checker reads the rendered HTML and source links. `[hooks.py:on_pre_build]` `[hooks.py:on_page_markdown]` `[check.py:source_links_check]`

## Invariants

The bundle collects licenses of included packages and the build fails if a license notice is missing. `[build.mjs:notices]`

## Side effects

Generation writes SVGs, JavaScript, CSS and third-party notices into site assets. Generated SVGs that leave the diagram set are deleted. `[hooks.py:on_pre_build]` `[build.mjs:writeFile]`

## Verification

From the repository root, run `mkdocs build --strict && python scripts/docs/check.py`.
