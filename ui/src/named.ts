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
export interface Created { id: string; version: string; created: boolean; diagnostics?: ConfigurationFinding[] }
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

/**
 * Whether a text is a valid name, description or message: bounded, no control character, no space at either end.
 * `White_Space` is Rust's `char::is_whitespace`, so the browser and `meada-core` agree (`trim()` also strips U+FEFF).
 */
export function plain(text: string, min: number, max: number): boolean {
  return [...text].length >= min && [...text].length <= max && !/^\p{White_Space}|\p{White_Space}$/u.test(text) && !/\p{Cc}/u.test(text);
}

export function stateText(state: WorkspaceState, t: NamedCopy): string { return t[state]; }
/** Text, never a color alone. */
export function ingestionText(ingestion: Ingestion, t: NamedCopy): string { return t[ingestion]; }
export function pendingText(reason: string | undefined, t: NamedCopy): string {
  return reason === 'no_set' || reason === 'stale_set' || reason === 'missing_login' ? t[reason] : t.pendingUnknown;
}

/** Where an entry's configuration comes from: the repository's own `meada.toml` wins whole (ADR 0011), else the context's blocks. */
export type ConfigurationSource = 'repository' | 'context' | 'none';
export type Coverage = 'indexed' | 'pending_ingestion' | 'excluded';
export type DiagnosticCode = 'ignored_by_repository' | 'unknown_until_ingested' | 'excluded_no_sponsor' | 'peer_missing' | 'documents_not_indexed';
export interface Diagnostic { code: DiagnosticCode; detail?: string }
/** One entry of the effective-configuration view: computed by `meada-core`, never part of a manifest's text or of any id. */
export interface EffectiveEntry {
  repo: string; source: ConfigurationSource; blocks: Record<string, unknown>; ignored: string[]; coverage: Coverage; diagnostics: Diagnostic[];
}
/** `GET workspaces/<id>/effective` and `POST workspaces/effective`. */
export interface EffectiveView {
  workspace: string | null; generation: string | null; entries: EffectiveEntry[];
  version?: string; current?: boolean; base?: number; state?: WorkspaceState; hidden?: number;
  /** Whether the held generation recorded the context at this version: when not, these blocks reach queries after the next refresh. */
  recorded?: boolean;
}
/** A diagnostic of an accepted write, beside its result: an ignored declaration never looks applied. */
export interface ConfigurationFinding { repo: string; code: DiagnosticCode; detail?: string }
/** `POST workspaces/<id>/versions` and `.../apply` answers. */
export interface Written { base: number; version: string; state?: WorkspaceState; diagnostics?: ConfigurationFinding[] }

export function effectiveRoute(id: string, version?: string): string {
  return version ? `workspaces/${id}/effective?${new URLSearchParams({ version })}` : `workspaces/${id}/effective`;
}
/** The body of a preview of manifest text before it is saved: it stores nothing. */
export function previewBody(entries: string): string { return JSON.stringify({ entries }); }
export function saveBody(entries: string, message: string, base: number): string { return JSON.stringify({ entries, message, base }); }
export function applyBody(version: string, base: number): string { return JSON.stringify({ version, base }); }

export function sourceText(source: ConfigurationSource, t: NamedCopy): string { return t[`source_${source}`]; }
export function coverageText(coverage: Coverage, t: NamedCopy): string { return t[`coverage_${coverage}`]; }
/** A diagnostic in words, in text and never by color alone; `detail` is the block or the peer it is about. */
export function diagnosticText(diagnostic: Diagnostic, t: NamedCopy): string {
  const word = t[`diagnostic_${diagnostic.code}`];
  return diagnostic.detail ? `${word} (${diagnostic.detail})` : word;
}
/** A diagnostic of a write, as the view words it. */
export function findingText(finding: ConfigurationFinding, t: NamedCopy): string {
  return `${finding.repo}: ${diagnosticText(finding, t)}`;
}
