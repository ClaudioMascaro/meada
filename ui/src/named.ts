import type { NamedCopy } from './named-i18n';

/** `state`: `live` when the current generation records the workspace at its current version, `applied` when it is stored and its ingestion is pending. */
export type WorkspaceState = 'applied' | 'live';
/** An entry's ingestion: `pending` is never shown as ready. */
export type Ingestion = 'pending' | 'ready' | 'excluded';

export interface WorkspaceSummary {
  id: string; name: string; description: string; version: string; state: WorkspaceState;
  entries: number; operate: boolean; hidden?: number;
}
/** `GET workspaces`: the workspaces the session sees, a page at a time. */
export interface WorkspaceList { workspaces: WorkspaceSummary[]; next: string | null }
export interface WorkspaceEntry { repo: string; ingestion: Ingestion; blocks: Record<string, unknown> }
/** `GET workspaces/<id>`; `operators` and `hidden` only for an operator of the workspace. */
export interface WorkspaceDetail {
  id: string; name: string; description: string; created: string; version: string; base: number;
  state: WorkspaceState; entries: WorkspaceEntry[]; operate: boolean; hidden?: number;
  operators?: { subject: string; login: string | null }[];
}
export interface Created { id: string; version: string; created: boolean }
/** What a refused creation says: the identities the session's grant reads, and a count of the others. */
export interface Refusal { refused: { identity: string }[]; hidden: number }

/** The most workspaces a page asks for. */
export const PAGE = 25;
export const NAME_MAX = 80;
export const DESCRIPTION_MAX = 500;
export const MESSAGE_MAX = 500;

export function listRoute(after?: string): string {
  const query = new URLSearchParams({ limit: String(PAGE) });
  if (after) query.set('after', after);
  return `workspaces?${query}`;
}

/** The body of a creation: the manifest text is the canonical one `meada-core` produced. */
export function createBody(name: string, description: string, entries: string, message: string): string {
  return JSON.stringify({ name, description, entries, message });
}

/** Whether a text is a valid name or description: bounded, no control character, no space at either end. */
export function plain(text: string, min: number, max: number): boolean {
  return [...text].length >= min && [...text].length <= max && text === text.trim() && !/\p{Cc}/u.test(text);
}

export function stateText(state: WorkspaceState, t: NamedCopy): string { return t[state]; }
/** Text, never a color alone. */
export function ingestionText(ingestion: Ingestion, t: NamedCopy): string { return t[ingestion]; }
export function pendingText(reason: string | undefined, t: NamedCopy): string {
  return reason === 'no_set' || reason === 'stale_set' || reason === 'missing_login' ? t[reason] : t.pendingUnknown;
}
