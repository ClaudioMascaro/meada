import type { CatalogCopy } from './catalog-i18n';

export type CatalogReason = 'no_set' | 'stale_set' | 'missing_login';
export interface CatalogSet { finished: string; age_seconds: number; max_age_seconds: number }
export interface CatalogAnswer {
  state: 'ready' | 'pending'; reason: CatalogReason | null; set: CatalogSet | null;
  repositories: { identity: string }[]; next: string | null;
}
export type CheckState = 'selectable' | 'unavailable' | 'pending';
export interface CheckAnswer {
  state: 'ready' | 'pending'; reason: CatalogReason | null; results: { identity: string; state: CheckState }[];
}

export const PAGE = 50;
export const CHECK_IDENTITIES = 50;
export const PREFIX_MAX = 128;

export function catalogRoute(q = '', after?: string): string {
  const query = new URLSearchParams({ limit: String(PAGE) });
  if (q) query.set('q', q);
  if (after) query.set('after', after);
  return `catalog?${query}`;
}

export function checkBody(identities: readonly string[]): string {
  return JSON.stringify({ identities: identities.slice(0, CHECK_IDENTITIES) });
}

export function reasonText(reason: CatalogReason | null, t: CatalogCopy): string {
  return reason === null ? '' : t[reason];
}

export function setText(set: CatalogSet | null, t: CatalogCopy): string {
  return set ? t.setAge(set.finished, set.age_seconds, set.max_age_seconds) : t.noSet;
}

export function checkText(state: CheckState, t: CatalogCopy): string {
  return t[state];
}
