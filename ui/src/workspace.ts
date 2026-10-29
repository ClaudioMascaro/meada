import { authorizeEntries, composeEntries, entriesJson, entriesText, init, validate, workspaceDiff, workspaceVersionId } from './core';
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

export interface Entry { repo: string; path: string; owner?: string; [key: string]: unknown }
export type Reason = 'outside_grant' | 'ownerless' | 'owner_change' | 'not_your_team';
export interface Refusal { identity: string; change: 'add' | 'edit' | 'remove'; reason: Reason }
/** What the session's token says it is: the teams' groups and what it reads. */
export interface Authority { groups: string[]; grant: '*' | string[] }
/** An entry as a view shows it: whether the session may change it, and why not. */
export interface EntryRow extends Entry { may_change: boolean; refusal?: Reason }

/** The flags a view adds to an entry. They never enter a text, an id or a payload. */
const ANNOTATIONS = ['may_change', 'refusal'];

export function plainEntries(entries: Record<string, unknown>[]): Entry[] {
  return entries.map(entry => Object.fromEntries(Object.entries(entry).filter(([key]) => !ANNOTATIONS.includes(key)))) as Entry[];
}

/** A version's entries as the editor's TOML, without any view annotation. */
export function versionText(repositories: Record<string, unknown>[]): string {
  return entriesText(JSON.stringify(plainEntries(repositories)));
}

export function authority(session: Session): Authority | undefined {
  return session.role === 'team_operator' ? { groups: session.groups ?? [], grant: session.grant ?? '*' } : undefined;
}

function refusals(who: Authority, current: Entry[], next: Entry[]): Refusal[] {
  return JSON.parse(authorizeEntries('team_operator', JSON.stringify(who.groups), JSON.stringify(who.grant),
    JSON.stringify(current), JSON.stringify(next))) as Refusal[];
}

/** Each held entry with the answer of `authorize_entries` to its removal, the change of that entry alone. */
export function entryRows(held: Entry[], who: Authority): EntryRow[] {
  const refused = new Map(refusals(who, held, []).map(refusal => [refusal.identity, refusal.reason]));
  return held.map(entry => ({ ...entry, may_change: !refused.has(entry.repo), ...refused.has(entry.repo) && { refusal: refused.get(entry.repo) } }));
}

/** The owners of the entries a diff touches (a reorder touches none), as the server's audit line names them. */
function touchedOwners(diff: WorkspaceDiff, before: Entry[], after: Entry[]): string[] {
  const owner = new Map([...before, ...after].map(entry => [entry.repo, entry.owner]));
  const touched = [...diff.added, ...diff.removed, ...diff.changed.filter(change => change.keys.some(key => key !== 'repository')).map(change => change.repo)];
  return [...new Set(touched.flatMap(repo => owner.get(repo) ?? []))].sort();
}

/**
 * A team operator's editor: the text holds the entries it reads. Each change is composed with the
 * current ones, diffed and authorized by the core's own functions, so the server's refusals show
 * before a save. The partial text's id is not the saved version's, so none is returned.
 */
export function teamEditorModel(text: string, held: Entry[], who: Authority, locale: Language) {
  const current = JSON.stringify(held);
  const base = editorModel(text, entriesText(current), locale);
  if (base.errors.length) return { errors: base.errors, diff: undefined, refusals: [] as Refusal[], owners: [] as string[] };
  const composed = JSON.parse(composeEntries(JSON.stringify(who.grant), current, entriesJson(text))) as Entry[];
  const diff = JSON.parse(workspaceDiff(entriesText(current), entriesText(JSON.stringify(composed)))) as WorkspaceDiff;
  return { errors: [], diff, refusals: refusals(who, held, composed), owners: touchedOwners(diff, held, composed) };
}

export interface VersionSummary { id: string; author: string; time: string; message: string }
export interface WorkspaceVersion extends VersionSummary { repositories: Record<string, unknown>[] }
export interface WorkspacePointer { id: string; fence: number | string }
export interface CurrentWorkspace { current: WorkspacePointer | null; version: WorkspaceVersion | null }
export interface WorkspaceVersions { current: WorkspacePointer | null; versions: VersionSummary[]; malformed: string[] }

/** A confirmed write names its CLI command (the global operator) or the teams that authorize it. */
export interface SaveAction {
  kind: 'save'; route: 'workspace/versions'; id?: string; command?: string; teams?: string[];
  body: { entries: string; message: string };
}
export interface ApplyAction {
  kind: 'apply'; route: 'workspace/apply'; rollback: boolean; command?: string; teams?: string[];
  body: { version: string };
}

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

/**
 * The apply a team operator is offered, `undefined` when `authorize_entries` over the entries it
 * reads, the current ones against the stored version's, refuses any; the API still decides.
 */
export function teamApplyAction(version: VersionSummary, current: string | undefined, versions: VersionSummary[],
  held: Entry[], stored: Entry[], who: Authority, locale: Language): ApplyAction | undefined {
  if (refusals(who, held, stored).length) return undefined;
  const diff = editorModel(versionText(stored), versionText(held), locale).diff;
  return { kind: 'apply', route: 'workspace/apply', body: { version: version.id },
    rollback: isRollback(version, current, versions), teams: diff ? touchedOwners(diff, held, stored) : [] };
}
