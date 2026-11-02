import { namedAuthorize, namedChanged, namedDiff, namedEntriesJson, resourceRoles } from './core';
import type { Entry, NamedDiff } from './guided';

/** What `namedAuthorize` decides: whether the grant covers the entries, and who it names when it does not. */
export interface Authorization { allowed: boolean; named: string[]; hidden: number }

// The one place a named-workspace core result is parsed (a call that only validates, and drops the result, stays raw): a field renamed in `meada-core` fails here, not in a page.
export const parseEntries = (text: string, max: number): Entry[] => JSON.parse(namedEntriesJson(text, max)) as Entry[];
export const diffOf = (before: Entry[], after: Entry[]): NamedDiff => JSON.parse(namedDiff(JSON.stringify(before), JSON.stringify(after))) as NamedDiff;
export const changedOf = (before: Entry[], after: Entry[]): string[] => JSON.parse(namedChanged(JSON.stringify(before), JSON.stringify(after))) as string[];
export const authorizationOf = (touched: string[], eligible: Iterable<string>, grant: string | string[]): Authorization =>
  JSON.parse(namedAuthorize(JSON.stringify(touched), JSON.stringify([...eligible]), JSON.stringify(grant))) as Authorization;
export const rolesOf = (): string[] => JSON.parse(resourceRoles()) as string[];
