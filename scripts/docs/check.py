#!/usr/bin/env python3
"""Check the emitted site and prove the public strict build rejects broken input."""
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import unquote, urlsplit
import json
import copy
import shutil
import re
import subprocess
import sys
import tempfile
import xml.etree.ElementTree as ET

import yaml
import markdown

ROOT = Path(__file__).resolve().parents[2]
SITE = ROOT / "site"


class Page(HTMLParser):
    def __init__(self, text):
        super().__init__()
        self.ids = set()
        self.links = []
        self.lang = None
        self.images = []
        self.feed(text)

    def handle_starttag(self, tag, pairs):
        attrs = dict(pairs)
        if "id" in attrs:
            self.ids.add(attrs["id"])
        if tag == "html":
            self.lang = attrs.get("lang")
        if tag == "img":
            self.images.append(attrs)
        for attr in ("href", "src"):
            if attrs.get(attr):
                self.links.append(attrs[attr])


def site_check():
    pages = {p: Page(p.read_text()) for p in SITE.rglob("*.html")}
    assert pages, "Run mkdocs build --strict first"
    errors = []
    for path, page in pages.items():
        for raw in page.links:
            url = urlsplit(raw)
            if url.scheme or url.netloc:
                continue
            target = unquote(url.path)
            if target.startswith("/"):
                if not target.startswith("/meada/"):
                    errors.append(f"{path}: URL leaves the project base: {raw}")
                    continue
                resolved = SITE / target.removeprefix("/meada/")
            else:
                resolved = (path.parent / target).resolve() if target else path
            if resolved.is_dir():
                resolved /= "index.html"
            if not resolved.exists():
                errors.append(f"{path}: missing target {raw}")
            elif url.fragment and resolved.suffix == ".html":
                destination = pages.get(resolved)
                if destination and unquote(url.fragment) not in destination.ids:
                    errors.append(f"{path}: missing anchor {raw}")
        for img in page.images:
            if "assets/diagrams/" in img.get("src", "") and not img.get("alt"):
                errors.append(f"{path}: diagram lacks alternative text")
    for locale, lang, prefix in (("pt", "pt-BR", ""), ("en", "en", "en/"), ("es", "es", "es/")):
        for source in (ROOT / "docs/site" / locale).glob("*.md"):
            relative = "index.html" if source.stem == "index" else f"{source.stem}/index.html"
            output = SITE / prefix / relative
            if output not in pages or pages[output].lang != lang:
                errors.append(f"{output}: missing translation or wrong html language (expected {lang})")
    diagrams = list((SITE / "assets/diagrams").glob("*.svg"))
    graphs = json.loads((ROOT / "docs/site/assets/graphs.json").read_text())["graphs"]
    expected = {f"{locale}-{key}.svg" for locale in ("pt", "en", "es") for key in graphs}
    if {p.name for p in diagrams} != expected:
        errors.append("Missing or unexpected localized graph fallback")
    ns = {"svg": "http://www.w3.org/2000/svg"}
    for diagram in diagrams:
        svg = ET.parse(diagram).getroot()
        if svg.find("svg:title", ns) is None or svg.find("svg:desc", ns) is None:
            errors.append(f"{diagram}: SVG lacks accessible title or description")
    for path in pages:
        text = path.read_text()
        payloads = re.findall(r'<script type="application/json">(.*?)</script>', text)
        graph_count = text.count('class="graph-explorer"')
        if graph_count != len(payloads):
            errors.append(f"{path}: graph payload/fallback mismatch")
        if text.count('type="module" src=') != bool(graph_count):
            errors.append(f"{path}: graph bundle must load once, only on graph pages")
        for payload in payloads:
            graph = json.loads(payload)['graph']
            for node in graph['nodes']:
                # Every explanatory detail is present outside the React root, even without JS.
                from html import escape
                if f'<span>{escape(node["detail"])}</span>' not in text:
                    errors.append(f"{path}: incomplete semantic fallback for {node['id']}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"PASS: {len(pages)} HTML pages, local URLs/anchors, three languages and {len(diagrams)} accessible graph fallbacks")


def source_links_check():
    sources = list((ROOT / "docs/site").rglob("*.md"))
    sources += [ROOT / name for name in (
        "README.md", "README.en.md", "README.es.md", "CODE_OF_CONDUCT.md",
        "CODE_OF_CONDUCT.en.md", "CODE_OF_CONDUCT.es.md", "CODING_STANDARDS.md",
        "CONTRIBUTING.md", "CONTRIBUTING.en.md", "CONTRIBUTING.es.md")]
    errors = []
    checked = 0
    for source in sources:
        for raw in re.findall(r"\]\(([^\s)]+)\)", source.read_text()):
            url = urlsplit(raw)
            if raw.startswith("https://github.com/ClaudioMascaro/meada/blob/main/"):
                target = ROOT / unquote(url.path.split("/blob/main/", 1)[1])
            elif raw.startswith("https://claudiomascaro.github.io/meada/"):
                target = SITE / unquote(url.path.removeprefix("/meada/"))
                if target.is_dir():
                    target /= "index.html"
            elif not url.scheme and not url.netloc:
                target = (source.parent / unquote(url.path)).resolve() if url.path else source
            else:
                continue
            checked += 1
            if not target.exists():
                errors.append(f"{source}: missing repository/site target {raw}")
            elif url.fragment and target.suffix == ".md":
                rendered = markdown.markdown(target.read_text(), extensions=["toc", "fenced_code"])
                if unquote(url.fragment) not in Page(rendered).ids:
                    errors.append(f"{source}: missing repository anchor {raw}")
    if errors:
        raise SystemExit("\n".join(errors))
    print(f"PASS: {checked} source links, including migrated repository references and README callers")


def rejection_checks():
    with tempfile.TemporaryDirectory(prefix="meada-docs-check-") as temp:
        temp = Path(temp)
        shutil.copytree(ROOT / "docs/site", temp / "docs")
        config = yaml.safe_load((ROOT / "mkdocs.yml").read_text())
        config.update(docs_dir=str(temp / "docs"), site_dir=str(temp / "site"))
        config["hooks"] = [str(ROOT / "scripts/docs/hooks.py")]
        config_file = temp / "mkdocs.yml"
        config_file.write_text(yaml.safe_dump(config, allow_unicode=True, sort_keys=False))
        source = temp / "docs/pt/index.md"
        original = source.read_text()
        graph_file = temp / "docs/assets/graphs.json"
        baseline = json.loads(graph_file.read_text())
        cases = [
            ("missing page", "definitely-missing.md", "\n[Missing](definitely-missing.md)\n", None),
            ("missing anchor", "definitely-missing-anchor", "\n[Missing](installation.md#definitely-missing-anchor)\n", None),
            ("unknown graph", "unknown graph reference", "\n<!-- meada-graph:missing -->\n", None),
            ("duplicate node", "duplicate node ID", "", lambda g: g['refresh']['nodes'].append(g['refresh']['nodes'][0])),
            ("dangling edge", "unknown edge endpoint", "", lambda g: g['refresh']['edges'][0].update(target='absent')),
            ("missing translation", "complete pt/en/es", "", lambda g: g['refresh']['title'].pop('es')),
            ("nonfinite layout", "finite nonnegative", "", lambda g: g['refresh']['nodes'][0]['position'].update(x=float('inf'))),
            ("roadmap cycle", "dependency cycle", "", lambda g: g['roadmap']['edges'][0].update(target=g['roadmap']['edges'][0]['source'])),
            ("missing graph section", "missing graph section anchor", "", lambda g: g['refresh']['nodes'][0]['reference'].update(pt='#absent')),
        ]
        for name, diagnostic, addition, mutate in cases:
            source.write_text(original + addition)
            document = copy.deepcopy(baseline)
            if mutate:
                mutate(document['graphs'])
            graph_file.write_text(json.dumps(document))
            result = subprocess.run([sys.executable, "-m", "mkdocs", "build", "--strict", "-f", str(config_file)],
                                    cwd=ROOT, capture_output=True, text=True)
            log = result.stdout + result.stderr
            if result.returncode == 0 or diagnostic not in log:
                raise SystemExit(f"FAIL: {name} was not rejected for its expected reason\n{log}")
            print(f"PASS: mkdocs build --strict rejects {name} (exit {result.returncode})")


if __name__ == "__main__":
    site_check()
    source_links_check()
    rejection_checks()
