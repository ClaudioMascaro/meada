import { useCallback, useEffect, useState } from 'react';
import type { Api } from './api';
import { catalogRoute, checkBody } from './catalog';
import type { CatalogAnswer, CheckAnswer, CheckState } from './catalog';

export interface CatalogView {
  /** The newest page's answer: `pending` says why nothing is selectable. */
  answer?: CatalogAnswer;
  /** The repositories listed now (the prefix searched, the pages read), in the server's order. */
  listed: string[];
  /** Every identity the server said the session may select since the last reload: a search never forgets one. */
  eligible: ReadonlySet<string>;
  /** What `catalog/check` said of the identities verified since the last reload. */
  checked: ReadonlyMap<string, CheckState>;
  problem?: unknown;
  search: (prefix: string) => void;
  more: () => void;
  /** Reads the catalog again and drops what was verified: the roles may have changed. */
  reload: () => void;
  verify: (identities: readonly string[]) => Promise<void>;
}

/**
 * The catalog the console serves (#341), read a page at a time. The selection rules are not here:
 * the server lists only what the session may select and `meada-core` marks a selection against it.
 */
export function useCatalog(api: Api): CatalogView {
  const [prefix, setPrefix] = useState('');
  const [after, setAfter] = useState<string>();
  const [attempt, setAttempt] = useState(0);
  const [answer, setAnswer] = useState<CatalogAnswer>();
  const [listed, setListed] = useState<string[]>([]);
  const [eligible, setEligible] = useState<ReadonlySet<string>>(new Set());
  const [checked, setChecked] = useState<ReadonlyMap<string, CheckState>>(new Map());
  const [problem, setProblem] = useState<unknown>();
  useEffect(() => {
    const controller = new AbortController();
    api.request<CatalogAnswer>(catalogRoute(prefix, after), { signal: controller.signal })
      .then(value => {
        if (controller.signal.aborted) return;
        const identities = value.repositories.map(r => r.identity);
        setProblem(undefined);
        setAnswer(value);
        setListed(previous => after === undefined ? identities : [...previous, ...identities]);
        setEligible(previous => new Set([...previous, ...identities]));
      })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, prefix, after, attempt]);
  const search = useCallback((next: string) => { setAfter(undefined); setListed([]); setPrefix(next); }, []);
  const more = useCallback(() => { if (answer?.next) setAfter(answer.next); }, [answer]);
  const reload = useCallback(() => {
    setAfter(undefined); setAnswer(undefined); setListed([]); setEligible(new Set()); setChecked(new Map()); setAttempt(value => value + 1);
  }, []);
  const verify = useCallback(async (identities: readonly string[]) => {
    const result = await api.request<CheckAnswer>('catalog/check', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: checkBody(identities) });
    setChecked(previous => new Map([...previous, ...result.results.map(r => [r.identity, r.state] as const)]));
    setEligible(previous => new Set([...previous, ...result.results.filter(r => r.state === 'selectable').map(r => r.identity)]));
  }, [api]);
  return { answer, listed, eligible, checked, problem, search, more, reload, verify };
}
