import { expect, test } from 'vitest';
import fleet from './fixtures/fleet.json';
import operational from './fixtures/operational.json';
import { fleetModel, storeModel, repositoriesModel, credentialsModel, checkModel, admissionModel } from '../src/operations';
import { operationsCopy } from '../src/operations-i18n';

// The API, rather than the browser, decides which validated heartbeats diagnose a stuck fleet.
test('fleet separates malformed heartbeats and names the stuck node and fleet recoveries', () => {
  const model = fleetModel(fleet, operationsCopy.en);
  expect(model.nodes).toHaveLength(1);
  expect(model.nodes[0]).toMatchObject({ name: 'node-a', fence: '15', readiness: 'Ready' });
  expect(model.nodes[0]?.recoveries).toEqual([
    'meada snapshot fence recover', "delete this node's <local_dir>/current",
  ]);
  expect(model.recoveries).toEqual(['meada snapshot fence recover']);
  expect(model.malformed).toEqual([{ name: 'bad/node', reason: 'invalid node name' }]);
  expect(model.retained).toContainEqual({ id: 'aaaaaaaaaaaaaaaa', fence: '15', retention: 'Kept manually' });
  expect(fleetModel({ requires: ['generations'] }, operationsCopy.en).missing).toBe('Requires [generations].');
});

test('admission preserves each independent check outcome and the admitted identities', () => {
  const model = checkModel({ checks: { signature: true, issuer: false, audience: null, lifetime: null, scopes: null }, identities: null }, operationsCopy.en);
  expect(model.checks).toContainEqual({ name: 'Signature', outcome: 'Passed' });
  expect(model.checks).toContainEqual({ name: 'Issuer', outcome: 'Failed' });
  expect(model.checks).toContainEqual({ name: 'Audience', outcome: 'Not checked' });
  expect(model.identities).toBeNull();
});

test('admission shows the identity provider type beside the issuer', () => {
  for (const locale of ['en', 'pt-BR'] as const) {
    const t = operationsCopy[locale];
    const fields = admissionModel(operational.admission, t);
    expect(fields.slice(0, 2)).toEqual([[t.issuer, 'https://issuer.example'], [t.providerType, 'entra']]);
  }
  expect(operationsCopy['pt-BR'].providerType).toBe('Tipo do provedor de identidade');
});

test('credentials show cached lifetime and throttle end, or name the missing GitHub App section', () => {
  const model = credentialsModel({ apps: [{ host: 'github.com', app_id: 42, malformed: false,
    throttled_until: null, installations: [{ owner: 'team', id: 7, cached: true,
      expires: 1700000600, remaining_seconds: 600, throttled_until: 1700000300 }] }] }, operationsCopy.en);
  expect(model.apps[0]?.installations[0]).toMatchObject({ cached: 'Cached', remaining: '600 s', throttle: '2023-11-14T22:18:20.000Z' });
  expect(credentialsModel({ requires: ['github_app'] }, operationsCopy.en).missing).toBe('Requires [[github_app]].');
});

test('repositories distinguish an index behind HEAD from a served generation and an unknown schema', () => {
  const model = repositoriesModel({ repositories: [
    { identity: 'example.com/team/old', head: 'new-commit', indexed_commit: 'old-commit', schema: 7, behind: true },
    { identity: 'example.com/team/api', generation: 'bbbbbbbbbbbbbbbb', commit: 'a1b2c3', schema: null, nodes: ['node-a'] },
  ] }, operationsCopy.en);
  expect(model.rows[0]).toMatchObject({ status: 'Index behind HEAD', head: 'new-commit', commit: 'old-commit' });
  expect(model.rows[1]).toMatchObject({ generation: 'bbbbbbbbbbbbbbbb', commit: 'a1b2c3', schema: 'Unknown', nodes: 'node-a' });
});

test('store exposes bundle size, hash and current/retained/kept marks, leaving older documents unloaded', () => {
  const model = storeModel({ generations: [
    { id: 'bbbbbbbbbbbbbbbb', current: true, retained: true, kept: true, document: {
      generation: 'bbbbbbbbbbbbbbbb', repositories: [{ identity: 'example.com/team/api', head: 'a1b2c3',
        bundle: 'bundles/aaaaaaaaaaaaaaaa.tar', size: 4096, blake3: 'f'.repeat(64), blocks: {} }],
    } },
    { id: 'cccccccccccccccc', current: false, retained: false, kept: false },
  ], malformed: [], pointer: null, retained: [] }, operationsCopy.en);
  expect(model.generations[0]?.retention).toEqual(['Current', 'Retained', 'Kept manually']);
  expect(model.generations[0]?.bundles[0]).toMatchObject({ identity: 'example.com/team/api', size: '4096', hash: 'f'.repeat(64) });
  expect(model.generations[1]?.loaded).toBe(false);
  expect(model.generations[1]?.bundles).toEqual([]);
});
