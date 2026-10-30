import type { CatalogCopy } from './catalog-i18n';

/** Why nothing is selectable: a condition of the platform or of the caller, the same for every repository. */
export type CatalogReason = 'no_set' | 'stale_set' | 'missing_login';
export interface CatalogSet { finished: string; age_seconds: number; max_age_seconds: number }
/** `GET catalog`: only the repositories the session may select, in order, a page at a time. */
export interface CatalogAnswer {
  state: 'ready' | 'pending'; reason: CatalogReason | null; set: CatalogSet | null;
  repositories: { identity: string }[]; next: string | null;
}
export type CheckState = 'selectable' | 'unavailable' | 'pending';
/** `POST catalog/check`: one state per identity asked, with no reason for an unavailable one. */
export interface CheckAnswer {
  state: 'ready' | 'pending'; reason: CatalogReason | null; results: { identity: string; state: CheckState }[];
}

/** The most repositories a page asks for. */
export const PAGE = 50;
/** The most identities one check takes. */
export const CHECK_IDENTITIES = 50;
/** The longest prefix the server takes. */
export const PREFIX_MAX = 128;

/** The route of a page: the prefix `q` and the cursor `after` the previous page gave, when there are any. */
export function catalogRoute(q = '', after?: string): string {
  const query = new URLSearchParams({ limit: String(PAGE) });
  if (q) query.set('q', q);
  if (after) query.set('after', after);
  return `catalog?${query}`;
}

/** The body of a check of `identities`, at most `CHECK_IDENTITIES` of them. */
export function checkBody(identities: readonly string[]): string {
  return JSON.stringify({ identities: identities.slice(0, CHECK_IDENTITIES) });
}

/** What the page says of a pending catalog, in text. */
export function reasonText(reason: CatalogReason | null, t: CatalogCopy): string {
  return reason === null ? '' : t[reason];
}

/** The set's age as the page shows it. */
export function setText(set: CatalogSet | null, t: CatalogCopy): string {
  return set ? t.setAge(set.finished, set.age_seconds, set.max_age_seconds) : t.noSet;
}

/** The text of a check result: never a color alone. */
export function checkText(state: CheckState, t: CatalogCopy): string {
  return t[state];
}
