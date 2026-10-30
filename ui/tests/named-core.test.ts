// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import table from '../../tests/fixtures/named-workspaces.json';
import effectiveTable from '../../tests/fixtures/effective-configuration.json';
import { initSync, init, namedAuthorize, namedEntriesJson, namedEffective, namedEntriesText, namedVersionId, namedVisible } from '../wasm/meada_core';

// The Y/A/B table (ADR 0020) the Rust tests read, through the actual WebAssembly bundle: the browser
// marks what a selection may not include exactly as the server's `meada_core::named` decides it.
type Caller = { rows: { identity: string; role: string }[]; claims_grant?: string[]; platform_administrator?: boolean; login: string | null };
const callers = table.callers as Record<string, Caller>;
const identities = table.identities as Record<string, string>;
const json = JSON.stringify;

function eligible(name: string): string[] {
  const caller = callers[name]!;
  if (caller.login === null) return [];
  return caller.rows.filter(row => table.accepted_roles.includes(row.role)).map(row => row.identity);
}
function grant(name: string): string[] {
  return [...new Set([...eligible(name), ...(callers[name]!.claims_grant ?? [])])];
}

beforeAll(() => {
  initSync({ module: readFileSync(new URL('../wasm/meada_core_bg.wasm', import.meta.url)) });
  init('en');
});

test.each(table.creation.filter(row => !callers[row.caller]!.platform_administrator))('creation: $caller selects $select', row => {
  const got = JSON.parse(namedAuthorize(json(row.select), json(eligible(row.caller)), json(grant(row.caller)))) as { allowed: boolean; named: string[]; hidden: number };
  expect(got.allowed).toBe(row.allowed);
  expect([...got.named].sort()).toEqual([...(row.named ?? [])].sort());
  if (row.hidden !== undefined) expect(got.hidden).toBe(row.hidden);
});

test.each(table.discovery)('discovery: $caller sees $workspace: $visible', row => {
  const workspace = (table.workspaces as Record<string, { entries: string[]; operators: string[] }>)[row.workspace]!;
  const operates = !!callers[row.caller]!.platform_administrator || workspace.operators.includes(row.caller);
  expect(namedVisible(operates, json(workspace.entries), json(grant(row.caller)))).toBe(row.visible);
});

test('a named text is validated and written canonically, with no path, owner or added_by', () => {
  const y = identities.Y!, a = identities.A!;
  const text = namedEntriesText(json([{ repo: y }, { repo: a }]));
  expect(text).toContain(`repo = "${y}"`);
  expect(text).not.toContain('path');
  expect(JSON.parse(namedEntriesJson(text, 5))).toEqual([{ repo: y }, { repo: a }]);
  expect(namedVersionId(text, 5)).toMatch(/^[0-9a-f]{16}$/);
  // Whitespace and key order are not the version: the same entries are one version.
  expect(namedVersionId(`[[repository]]\n\nrepo = "${y}"\n\n\n[[repository]]\nrepo = "${a}"\n`, 5)).toBe(namedVersionId(text, 5));
  for (const bad of [`[[repository]]\nrepo = "${y}"\npath = "p"\n`, `[[repository]]\nrepo = "${y}"\nowner = "g"\n`]) {
    expect(() => namedEntriesJson(bad, 5)).toThrow(/reserved/);
  }
  expect(() => namedEntriesJson(text, 1)).toThrow();
});

// The effective-configuration table (ADR 0021 contract 6) the Rust tests read, through the actual
// WebAssembly bundle: the browser and the server decide which source wins, what is ignored and what
// is not yet in effect with one function.
const ids = effectiveTable.identities as Record<string, string>;
type Row = { name: string; entries: Record<string, unknown>[]; facts: Record<string, Record<string, boolean>>;
  expect: { repo: string; source: string; coverage: string; ignored: string[]; diagnostics: string[] }[] };
const spelled = (value: unknown): unknown => {
  if (typeof value === 'string') return ids[value] ?? value;
  if (Array.isArray(value)) return value.map(spelled);
  if (value && typeof value === 'object') return Object.fromEntries(Object.entries(value).map(([k, v]) => [ids[k] ?? k, spelled(v)]));
  return value;
};
const view = (entries: unknown, facts: unknown) =>
  JSON.parse(namedEffective(json(spelled(entries)), json(spelled(facts)))) as { repo: string; source: string; coverage: string; ignored: string[]; blocks: unknown; diagnostics: { code: string }[] }[];

test.each(effectiveTable.rows as unknown as Row[])('effective configuration: $name', row => {
  const got = view(row.entries, row.facts);
  expect(got.map(entry => ({ repo: entry.repo, source: entry.source, coverage: entry.coverage, ignored: entry.ignored, diagnostics: entry.diagnostics.map(d => d.code) })))
    .toEqual(row.expect.map(want => ({ ...want, repo: ids[want.repo] })));
});

test('two contexts sharing a repository read only their own blocks, and the view is not part of a manifest', () => {
  const row = effectiveTable.contexts[0]!;
  const first = view(row.first, row.facts)[0]!, second = view(row.second, row.facts)[0]!;
  expect(first.blocks).toEqual(spelled(row.blocks.first));
  expect(second.blocks).toEqual(spelled(row.blocks.second));
  // The annotations are computed and never enter canonical text or an id: the text of the entries is the same with or without them.
  const text = namedEntriesText(json(spelled(row.first)));
  expect(text).not.toMatch(/source|coverage|diagnostic|ignored/);
  expect(namedVersionId(text, 5)).toBe(namedVersionId(namedEntriesText(json(JSON.parse(namedEntriesJson(text, 5)))), 5));
});
