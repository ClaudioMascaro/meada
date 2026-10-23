import { expect, test } from 'vitest';
import { queryBody } from '../src/playground';

test('search and context use their MCP arguments and leave the default generation implicit', () => {
  expect(queryBody('example.com/chain/web', '', 'search', 'orders', { kind: 'contract', direction: 'up', budget: '3000' }))
    .toEqual({ repository: 'example.com/chain/web', tool: 'search', arguments: { query: 'orders', kind: 'contract', budget: 3000 } });
  expect(queryBody('example.com/chain/web', '', 'context', 'dashboard', { kind: 'contract', direction: 'up', budget: '' }))
    .toEqual({ repository: 'example.com/chain/web', tool: 'context', arguments: { symbol: 'dashboard' } });
});
