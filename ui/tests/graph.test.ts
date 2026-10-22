// @vitest-environment node
import { readFileSync } from 'node:fs';
import { beforeAll, expect, test } from 'vitest';
import { initSync, init } from '../wasm/meada_core';
import { graphModel } from '../src/graph';

beforeAll(() => {
  initSync({ module: readFileSync('wasm/meada_core_bg.wasm') });
  init('en');
});
const golden = (name: string) => readFileSync(`../tests/golden/${name}.export.json`, 'utf8');

test('communities retain the chain golden members', () => {
  const graph = graphModel(golden('chain-web'));
  expect(graph.communities).toHaveLength(3);
  expect(graph.communities[0]).toEqual({
    name: 'src', members: [
      'example.com/chain/web::src/api/client.ts::request',
      'example.com/chain/web::src/orders.ts::createOrder',
      'example.com/chain/web::src/orders.ts::listOrders',
    ],
  });
});

test('contracts group each concrete contract with its provider and consumer sites', () => {
  const graph = graphModel(golden('chain-catalog'));
  expect(graph.contracts).toContainEqual({
    contract: 'http::GET::/api/products/{}',
    providers: ['example.com/chain/catalog::app/api/products/[id]/route.ts::GET'],
    consumers: [],
  });
  expect(graph.contracts).toContainEqual({
    contract: 'resource::postgres::orders', providers: [], consumers: ['example.com/chain/catalog'],
  });
});

test('links include only cross-repository consumers and providers with their levels', () => {
  const graph = graphModel(golden('chain-web'));
  expect(graph.links).toHaveLength(5);
  expect(graph.links[0]).toMatchObject({
    consumer: 'example.com/chain/web::src/dashboard.ts::cancel::cancelOrder#0',
    provider: 'example.com/chain/gateway::src/orders.ts::cancelOrder', level: 'inferred',
  });
});

test('a link whose sites belong to the same repository is excluded from cross-repository links', () => {
  const data = JSON.parse(golden('chain-web'));
  data.links.push({
    consumer: 'example.com/chain/web::local.ts::caller', provider: 'example.com/chain/web::local.ts::provider',
    contract: 'http::GET::/local', level: 'exact', reason: 'same repository',
  });
  expect(graphModel(JSON.stringify(data)).links).toHaveLength(5);
});

test('shape drift retains missing paths with their levels and unchecked paths with their open part', () => {
  const graph = graphModel(golden('chain-drift-catalog'));
  expect(graph.drift).toContainEqual(expect.objectContaining({
    consumer: 'example.com/chain/web::src/comments.ts::comments::fetch#0',
    missing: { '[].rating': 'inferred' }, unchecked: { '[].meta.stars': '[].meta' },
  }));
});

test('decisions retain the decider and probability, and stale decisions retain their cause', () => {
  const graph = graphModel(golden('chain-web.decided'));
  expect(graph.decisions).toHaveLength(2);
  expect(graph.decisions).toContainEqual(expect.objectContaining({ by: 'human' }));
  expect(graph.decisions).toContainEqual(expect.objectContaining({ by: 'openrouter/jev', p: 0.62 }));
  const stale = graphModel(golden('chain-web.stale-call-gone')).stale;
  expect(stale).toContainEqual({
    call: 'example.com/chain/web::src/status.ts::health::fetch#0',
    target: 'example.com/chain/gateway::http::GET::/api/health', cause: 'call-gone',
  });
});

test('an invalid export is refused with the core parse error before building any view', () => {
  expect(() => graphModel('{"repository":"x","files":[{"path":"a","hash":7}]}'))
    .toThrow('files[0].hash');
});
