import { parseExport } from './core';

interface Community { name: string; members: string[] }
interface Side { contract: string | null; site: string }
interface Link {
  consumer: string; provider: string; contract: string; level: string; reason: string;
  by?: string; p?: number; drift?: { missing?: Record<string, string>; unchecked?: Record<string, string> };
}
interface Stale { call: string; target: string; cause: string }
interface Export { repository: string; communities?: Community[]; providers: Side[]; consumers: Side[]; links: Link[]; stale_decisions?: Stale[] }
interface Contract { contract: string; providers: string[]; consumers: string[] }

/** parseExport checks every field with the release's Rust types before any model is built. */
export function graphModel(text: string) {
  const data = JSON.parse(parseExport(text)) as Export;
  const contracts = new Map<string, Contract>();
  for (const side of ['providers', 'consumers'] as const) {
    for (const entry of data[side]) {
      if (entry.contract === null) continue;
      let row = contracts.get(entry.contract);
      if (!row) {
        row = { contract: entry.contract, providers: [], consumers: [] };
        contracts.set(entry.contract, row);
      }
      row[side].push(entry.site);
    }
  }
  return {
    repository: data.repository,
    communities: data.communities ?? [],
    contracts: [...contracts.values()].sort((a, b) => a.contract < b.contract ? -1 : a.contract > b.contract ? 1 : 0),
    links: data.links.filter(link => link.consumer.split('::')[0] !== link.provider.split('::')[0]),
    drift: data.links.filter(link => link.drift).map(link => ({
      consumer: link.consumer, provider: link.provider, contract: link.contract,
      missing: link.drift?.missing ?? {}, unchecked: link.drift?.unchecked ?? {},
    })),
    decisions: data.links.filter(link => link.level === 'decided'),
    stale: data.stale_decisions ?? [],
  };
}

export type GraphModel = ReturnType<typeof graphModel>;
