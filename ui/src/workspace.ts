import { authorizeEntries, composeEntries, entriesJson, entriesText, init, touchedOwners, validate, workspaceDiff, workspaceVersionId } from './core';
import type { Session } from './api';
import type { Language } from './i18n';
import { shellQuote } from './commands';

export interface WorkspaceDiff { added: string[]; removed: string[]; changed: { repo: string; keys: string[] }[] }

export function editorModel(text: string, current: string, locale: Language) {
  init(locale);
  const errors = validate('entries', text);
  return {
    errors,
    id: errors.length ? undefined : workspaceVersionId(text),
    diff: errors.length ? undefined : JSON.parse(workspaceDiff(current, text)) as WorkspaceDiff,
  };
}

export interface Entry { repo: string; path: string; owner?: string; added_by?: string; added_by_time?: string; [key: string]: unknown }
const flagged = (flag: string) => (repositories: Record<string, unknown>[]): Set<string> =>
  new Set(repositories.filter(entry => entry[flag] === true).map(entry => String(entry.repo)));
export const ownerMissing = flagged('owner_missing');
export const excludedEntries = flagged('excluded');
export type Reason = 'outside_grant' | 'unattributed' | 'ownerless' | 'owner_change' | 'added_by_change' | 'not_your_team';
export interface Refusal { identity: string; change: 'add' | 'edit' | 'remove'; reason: Reason }
export interface Authority { groups: string[]; grant: '*' | string[]; login?: string; host?: string }
export interface EntryRow extends Entry { may_change: boolean; refusal?: Reason }

const ANNOTATIONS = ['may_change', 'refusal', 'owner_missing', 'excluded'];

export function plainEntries(entries: Record<string, unknown>[]): Entry[] {
  return entries.map(entry => Object.fromEntries(Object.entries(entry).filter(([key]) => !ANNOTATIONS.includes(key)))) as Entry[];
}

export function versionText(repositories: Record<string, unknown>[]): string {
  return entriesText(JSON.stringify(plainEntries(repositories)));
}

export function authority(session: Session): Authority | undefined {
  return session.role === 'team_operator'
    ? { groups: session.groups ?? [], grant: session.grant ?? '*', login: session.login, host: session.github_host }
    : undefined;
}

function refusals(who: Authority, current: Entry[], next: Entry[]): Refusal[] {
  return JSON.parse(authorizeEntries('team_operator', JSON.stringify(who.groups), JSON.stringify(who.grant), who.login, who.host,
    JSON.stringify(current), JSON.stringify(next))) as Refusal[];
}

export function entryRows(held: Entry[], who: Authority): EntryRow[] {
  const refused = new Map(refusals(who, held, []).map(refusal => [refusal.identity, refusal.reason]));
  return held.map(entry => ({ ...entry, may_change: !refused.has(entry.repo), ...refused.has(entry.repo) && { refusal: refused.get(entry.repo) } }));
}

const owners = (before: Entry[], after: Entry[]): string[] => JSON.parse(touchedOwners(JSON.stringify(before), JSON.stringify(after))) as string[];

export function teamEditorModel(text: string, held: Entry[], who: Authority, locale: Language) {
  const current = JSON.stringify(held);
  const base = editorModel(text, entriesText(current), locale);
  if (base.errors.length) return { errors: base.errors, diff: undefined, refusals: [] as Refusal[], owners: [] as string[] };
  const now = new Date().toISOString().slice(0, 19) + 'Z';
  const composed = JSON.parse(composeEntries(JSON.stringify(who.grant), who.login, who.host, now, current, entriesJson(text))) as Entry[];
  const diff = JSON.parse(workspaceDiff(entriesText(current), entriesText(JSON.stringify(composed)))) as WorkspaceDiff;
  return { errors: [], diff, refusals: refusals(who, held, composed), owners: owners(held, composed) };
}

export interface VersionSummary { id: string; author: string; time: string; message: string }
export interface WorkspaceVersion extends VersionSummary { repositories: Record<string, unknown>[] }
export interface WorkspacePointer { id: string; fence: number | string }
export interface CurrentWorkspace { current: WorkspacePointer | null; version: WorkspaceVersion | null }
export interface WorkspaceVersions { current: WorkspacePointer | null; versions: VersionSummary[]; malformed: string[] }

type Who = { command: string; teams?: never } | { teams: string[]; command?: never };
export type SaveAction = { kind: 'save'; route: 'workspace/versions'; id?: string; body: { entries: string; message: string } } & Who;
export type ApplyAction = { kind: 'apply'; route: 'workspace/apply'; rollback: boolean; body: { version: string } } & Who;

export function saveAction(entries: string, message: string, file: string): SaveAction {
  const id = workspaceVersionId(entries);
  if (id === undefined || !message.trim()) throw new TypeError('Invalid workspace save');
  return { kind: 'save', route: 'workspace/versions', id, body: { entries, message },
    command: `meada workspace import <file> --message ${shellQuote(message)} --workspace ${shellQuote(file)}` };
}

function isRollback(version: VersionSummary, current: string | undefined, versions: VersionSummary[]): boolean {
  const before = versions.findIndex(row => row.id === current);
  return before >= 0 && versions.findIndex(row => row.id === version.id) > before;
}

export function applyAction(version: VersionSummary, current: string | undefined, versions: VersionSummary[], file: string): ApplyAction {
  return { kind: 'apply', route: 'workspace/apply', body: { version: version.id }, rollback: isRollback(version, current, versions),
    command: `meada workspace apply ${shellQuote(version.id)} --workspace ${shellQuote(file)}` };
}

export function teamSaveAction(entries: string, message: string, teams: string[]): SaveAction {
  if (!message.trim()) throw new TypeError('Invalid workspace save');
  return { kind: 'save', route: 'workspace/versions', teams, body: { entries, message } };
}

export function teamApplyAction(version: VersionSummary, current: string | undefined, versions: VersionSummary[],
  held: Entry[], stored: Entry[], who: Authority): ApplyAction | undefined {
  if (refusals(who, held, stored).length) return undefined;
  return { kind: 'apply', route: 'workspace/apply', body: { version: version.id },
    rollback: isRollback(version, current, versions), teams: owners(held, stored) };
}

export function ownerAt(text: string, caret: number): { prefix: string; start: number } | undefined {
  const line = caret > 0 ? text.lastIndexOf('\n', caret - 1) + 1 : 0;
  const match = /^\s*owner\s*=\s*"((?:[^"\\\n]|\\.)*)$/.exec(text.slice(line, caret));
  if (!match) return undefined;
  const written = match[1] ?? '';
  let prefix = written;
  try { prefix = JSON.parse(`"${written}"`) as string; } catch {  }
  return { prefix, start: caret - written.length };
}

export const tomlEscaped = (value: string): string => JSON.stringify(value).slice(1, -1);
