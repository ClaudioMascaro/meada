import { namedAuthorize, namedChanged, namedDiff, namedEntriesJson, resourceRoles } from './core';
import type { Entry, NamedDiff } from './guided';

export interface Authorization { allowed: boolean; named: string[]; hidden: number }

export const parseEntries = (text: string, max: number): Entry[] => JSON.parse(namedEntriesJson(text, max)) as Entry[];
export const diffOf = (before: Entry[], after: Entry[]): NamedDiff => JSON.parse(namedDiff(JSON.stringify(before), JSON.stringify(after))) as NamedDiff;
export const changedOf = (before: Entry[], after: Entry[]): string[] => JSON.parse(namedChanged(JSON.stringify(before), JSON.stringify(after))) as string[];
export const authorizationOf = (touched: string[], eligible: Iterable<string>, grant: string | string[]): Authorization =>
  JSON.parse(namedAuthorize(JSON.stringify(touched), JSON.stringify([...eligible]), JSON.stringify(grant))) as Authorization;
export const rolesOf = (): string[] => JSON.parse(resourceRoles()) as string[];
