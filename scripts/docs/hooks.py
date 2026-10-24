"""Validate versioned graphs, build local islands, and preserve a readable static fallback."""
from base64 import b64encode
from hashlib import sha256
from html import escape
from pathlib import Path
import json
import math
import re
import subprocess
import textwrap

import markdown as md

from mkdocs.exceptions import PluginError
from mkdocs.utils import get_relative_url

ROOT = Path(__file__).resolve().parents[2]
LANGUAGES = ('pt', 'en', 'es')
MARKER = re.compile(r'<!-- meada-graph:([a-z][a-z0-9-]*) -->')
GRAPHS = {}
WIDTH, HEIGHT = 244, 168
LABELS = {
    'pt': dict(details='Etapas e relações em texto', open='Abrir imagem completa', licenses='Licenças dos componentes', next='Conecta a', delivered='Entregue', planned='Planejado', guardrail='Em caso de falha', spec='Especificação'),
    'en': dict(details='Steps and relationships in text', open='Open full image', licenses='Component licenses', next='Connects to', delivered='Delivered', planned='Planned', guardrail='On failure', spec='Specification'),
    'es': dict(details='Etapas y relaciones en texto', open='Abrir imagen completa', licenses='Licencias de componentes', next='Conecta con', delivered='Entregado', planned='Planeado', guardrail='En caso de fallo', spec='Especificación'),
}


def fail(message):
    raise PluginError(f'Graph validation: {message}')


def localized(value, context):
    if not isinstance(value, dict) or set(value) != set(LANGUAGES):
        fail(f'{context}: requires complete pt/en/es translations')
    if any(not isinstance(v, str) or not v.strip() or '\u2014' in v for v in value.values()):
        fail(f'{context}: translations must be nonempty text without em dashes')


def validate_graphs(path):
    try:
        document = json.loads(path.read_text())
    except (ValueError, OSError) as error:
        fail(f'cannot read graph data: {error}')
    if document.get('schemaVersion') != 1 or not isinstance(document.get('graphs'), dict):
        fail('unsupported graph schema')
    graphs = document['graphs']
    if not {'refresh', 'admission', 'workspaces', 'enterprise', 'roadmap'} <= set(graphs):
        fail('expected four operational graphs and the roadmap')
    for key, graph in graphs.items():
        for field in ('title', 'summary'):
            localized(graph.get(field), f'{key}.{field}')
        ids = set()
        for node in graph.get('nodes', []):
            identity = node.get('id')
            if not isinstance(identity, str) or not re.fullmatch(r'[a-z][a-z0-9-]*', identity) or identity in ids:
                fail(f'{key}: invalid or duplicate node ID {identity}')
            ids.add(identity)
            for field in ('title', 'body', 'detail', 'reference'):
                localized(node.get(field), f'{key}.{identity}.{field}')
            if any(not re.fullmatch(r'#[a-z0-9_-]+', value) for value in node['reference'].values()):
                fail(f'{key}.{identity}: reference must be a local section anchor')
            if node.get('kind') not in ('process', 'delivered', 'planned', 'guardrail'):
                fail(f'{key}.{identity}: unknown node kind')
            point = node.get('position', {})
            if set(point) != {'x', 'y'} or any(type(v) not in (int, float) or not math.isfinite(v) or v < 0 for v in point.values()):
                fail(f'{key}.{identity}: positions must be finite nonnegative coordinates')
            if 'issue' in node and (type(node['issue']) is not int or node['issue'] <= 0):
                fail(f'{key}.{identity}: invalid specification reference')
            if 'stage' in node and node['stage'] not in ('S1', 'S2', 'S3', 'S4', 'S5'):
                fail(f'{key}.{identity}: unknown stage')
        if not ids or graph.get('start') not in ids:
            fail(f'{key}: missing start node')
        edge_ids, successors = set(), {identity: [] for identity in ids}
        for edge in graph.get('edges', []):
            identity = edge.get('id')
            if not isinstance(identity, str) or identity in edge_ids:
                fail(f'{key}: duplicate edge ID {identity}')
            edge_ids.add(identity)
            if edge.get('source') not in ids or edge.get('target') not in ids:
                fail(f'{key}.{identity}: unknown edge endpoint')
            successors[edge['source']].append(edge['target'])
            if edge.get('kind') not in ('normal', 'planned', 'failure'):
                fail(f'{key}.{identity}: unknown edge kind')
            if any(edge.get(field) not in ('top', 'right', 'bottom', 'left') for field in ('sourceHandle', 'targetHandle')):
                fail(f'{key}.{identity}: invalid edge handle')
            if 'label' in edge:
                localized(edge['label'], f'{key}.{identity}.label')
        if key == 'roadmap':
            visited, visiting = set(), set()
            def visit(identity):
                if identity in visiting:
                    fail('roadmap: dependency cycle')
                if identity in visited:
                    return
                visiting.add(identity)
                for successor in successors[identity]:
                    visit(successor)
                visiting.remove(identity)
                visited.add(identity)
            for identity in ids:
                visit(identity)
    return graphs


def translated(graph, locale):
    result = dict(graph, title=graph['title'][locale], summary=graph['summary'][locale])
    result['nodes'] = [dict(n, **{field: n[field][locale] for field in ('title', 'body', 'detail', 'reference')}) for n in graph['nodes']]
    result['edges'] = [dict(e, **({'label': e['label'][locale]} if 'label' in e else {})) for e in graph['edges']]
    return result


def svg_fallback(graph, locale, font):
    nodes = {n['id']: n for n in graph['nodes']}
    width = max(n['position']['x'] for n in nodes.values()) + WIDTH + 48
    height = max(n['position']['y'] for n in nodes.values()) + HEIGHT + 144
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="-24 -120 {width} {height}" role="img" aria-labelledby="title desc">',
             f'<title id="title">{escape(graph["title"])}</title><desc id="desc">{escape(graph["summary"])}</desc>',
             f'<style>@font-face{{font-family:Plex;src:url(data:font/woff2;base64,{font}) format("woff2")}}text{{font-family:Plex,sans-serif}}</style>',
             '<defs><marker id="arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse"><path d="M0 0L10 5L0 10Z" fill="#587b68"/></marker></defs>']
    def point(node, side):
        x, y = node['position']['x'], node['position']['y']
        return {'left': (x, y + HEIGHT / 2), 'right': (x + WIDTH, y + HEIGHT / 2), 'top': (x + WIDTH / 2, y), 'bottom': (x + WIDTH / 2, y + HEIGHT)}[side]
    def control(p, side):
        dx, dy = {'left': (-90, 0), 'right': (90, 0), 'top': (0, -90), 'bottom': (0, 90)}[side]
        return p[0] + dx, p[1] + dy
    for edge in graph['edges']:
        start, end = point(nodes[edge['source']], edge['sourceHandle']), point(nodes[edge['target']], edge['targetHandle'])
        a, b = control(start, edge['sourceHandle']), control(end, edge['targetHandle'])
        color = {'normal': '#587b68', 'planned': '#927138', 'failure': '#a35a43'}[edge['kind']]
        dash = ' stroke-dasharray="6 5"' if edge['kind'] == 'planned' else ''
        parts.append(f'<path d="M{start[0]} {start[1]}C{a[0]} {a[1]} {b[0]} {b[1]} {end[0]} {end[1]}" stroke="{color}" stroke-width="1.5" fill="none" marker-end="url(#arrow)"{dash}/>')
        if edge.get('label'):
            x, y = (start[0] + end[0]) / 2, (start[1] + end[1]) / 2 - 8
            parts.append(f'<text x="{x}" y="{y}" text-anchor="middle" font-size="13" fill="#263d34" stroke="#fafbf8" stroke-width="4" paint-order="stroke">{escape(edge["label"])}</text>')
    for node in nodes.values():
        x, y = node['position']['x'], node['position']['y']
        stroke = '#927138' if node['kind'] == 'planned' else '#a35a43' if node['kind'] == 'guardrail' else '#c9d8cd'
        dash = ' stroke-dasharray="6 5"' if node['kind'] == 'planned' else ''
        parts.append(f'<rect x="{x}" y="{y}" width="{WIDTH}" height="{HEIGHT}" rx="12" fill="#fff" stroke="{stroke}"{dash}/>')
        titles = textwrap.wrap(node['title'], 23)
        for i, line in enumerate(titles):
            parts.append(f'<text x="{x + 16}" y="{y + 28 + i * 22}" font-size="18" fill="#263d34">{escape(line)}</text>')
        top = y + 39 + len(titles) * 22
        for i, line in enumerate(textwrap.wrap(node['body'], 30)):
            parts.append(f'<text x="{x + 16}" y="{top + i * 18}" font-size="14" fill="#53695b">{escape(line)}</text>')
        meta = ' · '.join(filter(None, [node.get('stage'), LABELS[locale].get(node['kind']), f'#{node["issue"]}' if node.get('issue') else None]))
        parts.append(f'<text x="{x + 16}" y="{y + 153}" font-size="11" fill="#53695b">{escape(meta)}</text>')
    return ''.join(parts) + '</svg>'


def on_pre_build(config):
    global GRAPHS
    docs = Path(config['docs_dir'])
    pages = {lang: {p.relative_to(docs / lang) for p in (docs / lang).rglob('*.md')} for lang in LANGUAGES}
    if not pages['pt'] or any(pages[lang] != pages['pt'] for lang in LANGUAGES):
        fail('documentation must have exactly the same page paths in pt/en/es')
    GRAPHS = validate_graphs(docs / 'assets/graphs.json')
    for lang in LANGUAGES:
        used = []
        for page in (docs / lang).rglob('*.md'):
            markdown = page.read_text()
            if '\u2014' in markdown:
                fail(f'{page}: em dash is not allowed')
            references = MARKER.findall(markdown)
            if markdown.count('<!-- meada-graph:') != len(references):
                fail(f'{page}: malformed graph reference')
            for reference in references:
                if reference not in GRAPHS:
                    fail(f'{page}: unknown graph reference {reference}')
                ids = set(re.findall(r' id="([^"]+)"', md.markdown(markdown, extensions=['toc', 'fenced_code'])))
                for node in GRAPHS[reference]['nodes']:
                    if node['reference'][lang][1:] not in ids:
                        fail(f'{page}: missing graph section anchor {node["reference"][lang]}')
            used.extend(references)
        if sorted(used) != sorted(GRAPHS):
            fail(f'{lang}: each graph must have one documentation home')
    output = docs / 'assets/interactive'
    output.mkdir(parents=True, exist_ok=True)
    recipe = sha256(b''.join((ROOT / 'scripts/docs' / name).read_bytes() for name in ('build.mjs', 'flow.jsx', 'flow.css', 'package-lock.json'))).hexdigest()
    stamp = output / '.build-hash'
    if not stamp.exists() or stamp.read_text() != recipe or any(not (output / name).exists() for name in ('flow.js', 'flow.css', 'THIRD_PARTY_NOTICES.txt')):
        result = subprocess.run(['node', str(ROOT / 'scripts/docs/build.mjs'), str(output)], cwd=ROOT, capture_output=True, text=True)
        if result.returncode:
            raise PluginError('Local graph bundle failed:\n' + result.stderr + result.stdout)
        stamp.write_text(recipe)
    diagrams = docs / 'assets/diagrams'
    diagrams.mkdir(parents=True, exist_ok=True)
    font = b64encode((docs / 'assets/fonts/ibm-plex-sans-latin-400-normal.woff2').read_bytes()).decode()
    expected = set()
    for locale in LANGUAGES:
        for key, graph in GRAPHS.items():
            name = f'{locale}-{key}.svg'
            expected.add(name)
            (diagrams / name).write_text(svg_fallback(translated(graph, locale), locale, font))
    for stale in diagrams.glob('*.svg'):
        if stale.name not in expected:
            stale.unlink()


def on_page_markdown(markdown, page, config, files):
    locale = {'pt-BR': 'pt', 'en': 'en', 'es': 'es'}[config.theme['language']]
    labels = LABELS[locale]
    def render(match):
        key = match[1]
        graph = translated(GRAPHS[key], locale)
        relative = lambda path: get_relative_url(path, page.file.url)
        image = relative(f'assets/diagrams/{locale}-{key}.svg')
        nodes = {n['id']: n for n in graph['nodes']}
        rows = []
        for node in graph['nodes']:
            status = f' ({labels[node["kind"]]})' if node['kind'] != 'process' else ''
            title = escape(f'{node.get("stage", "")} {node["title"]}{status}'.strip())
            if node.get('issue'):
                title += f' <a href="https://github.com/ClaudioMascaro/meada/issues/{node["issue"]}">#{node["issue"]}</a>'
            following = [nodes[e['target']]['title'] + (f' ({e["label"]})' if e.get('label') else '') for e in graph['edges'] if e['source'] == node['id']]
            relation = f'<span class="graph-next">{labels["next"]}: {escape("; ".join(following))}.</span>' if following else ''
            rows.append(f'<li><strong>{title}</strong><span>{escape(node["detail"])}</span>{relation}<a href="{escape(node["reference"])}">{labels["spec"]}</a></li>')
        payload = json.dumps(dict(graph=graph, locale=locale), ensure_ascii=False).replace('<', '\\u003c')
        return (f'<section class="graph-explorer" aria-label="{escape(graph["title"])}" data-graph="{key}">'
                f'<div class="graph-static"><figure class="diagram"><div class="diagram-scroll" tabindex="0" role="region" aria-label="{escape(graph["title"])}">'
                f'<a href="{image}"><img src="{image}" alt="{escape(graph["summary"])}" loading="lazy"></a></div><figcaption>{escape(graph["title"])}</figcaption></figure></div>'
                '<div class="meada-flow"></div>'
                f'<script type="application/json">{payload}</script>'
                f'<details class="graph-description" open><summary>{labels["details"]}</summary><ul>{"".join(rows)}</ul></details>'
                f'<p class="graph-links"><a href="{image}">{labels["open"]}</a><a href="{relative("assets/interactive/THIRD_PARTY_NOTICES.txt")}">{labels["licenses"]}</a></p></section>')
    rendered = MARKER.sub(render, markdown)
    if MARKER.search(markdown):
        css = get_relative_url('assets/interactive/flow.css', page.file.url)
        script = get_relative_url('assets/interactive/flow.js', page.file.url)
        rendered += f'\n<link rel="stylesheet" href="{css}"><script type="module" src="{script}"></script>\n'
    return rendered


def on_post_page(output, page, config):
    if config.theme['language'] == 'pt-BR':
        output = output.replace('<html lang="pt"', '<html lang="pt-BR"')
    output = output.replace('hreflang="pt"', 'hreflang="pt-BR"')
    # static-i18n already emits contextual language links. Material's alternate-link
    # enhancement assumes language roots and requests nonexistent per-page sitemaps.
    output = re.sub(r'<link[^>]+rel="alternate"[^>]*>', '', output)
    # Keep the repository link without fetching release statistics from GitHub.
    output = output.replace('data-md-component="source"', '')
    label = {'en': 'Change theme', 'es': 'Cambiar tema'}.get(config.theme['language'])
    return output.replace('Mudar tema', label) if label else output
