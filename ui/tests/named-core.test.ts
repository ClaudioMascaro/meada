// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import table from '../../tests/fixtures/named-workspaces.json';
import { initSync, init, namedAuthorize, namedEntriesJson, namedEntriesText, namedVersionId, namedVisible } from '../wasm/meada_core';

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
