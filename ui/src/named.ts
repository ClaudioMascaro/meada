import type { NamedCopy } from './named-i18n';

export type WorkspaceState = 'applied' | 'live';
export type Ingestion = 'pending' | 'ready' | 'excluded';

export interface WorkspaceSummary {
  id: string; name: string; description: string; version: string; state: WorkspaceState;
  entries: number; operate: boolean; hidden?: number;
}
export interface WorkspaceList { workspaces: WorkspaceSummary[]; next: string | null }
export interface WorkspaceEntry { repo: string; ingestion: Ingestion; blocks: Record<string, unknown> }
export interface WorkspaceDetail {
  id: string; name: string; description: string; created: string; version: string; base: number;
  state: WorkspaceState; entries: WorkspaceEntry[]; operate: boolean; hidden?: number;
  operators?: { subject: string; login: string | null }[];
}
export interface Created { id: string; version: string; created: boolean; diagnostics?: ConfigurationFinding[] }
export interface Refusal { refused: { identity: string }[]; hidden: number }

export const PAGE = 25;

export function listRoute(after?: string): string {
  const query = new URLSearchParams({ limit: String(PAGE) });
  if (after) query.set('after', after);
  return `workspaces?${query}`;
}

export function createBody(name: string, description: string, entries: string, message: string): string {
  return JSON.stringify({ name, description, entries, message });
}

export function stateText(state: WorkspaceState, t: NamedCopy): string { return t[state]; }
export function ingestionText(ingestion: Ingestion, t: NamedCopy): string { return t[ingestion]; }
export function pendingText(reason: string | undefined, t: NamedCopy): string {
  return reason === 'no_set' || reason === 'stale_set' || reason === 'missing_login' ? t[reason] : t.pendingUnknown;
}

export type ConfigurationSource = 'repository' | 'context' | 'none';
export type Coverage = 'indexed' | 'pending_ingestion' | 'excluded';
export type DiagnosticCode = 'ignored_by_repository' | 'unknown_until_ingested' | 'excluded_no_sponsor' | 'peer_missing' | 'documents_not_indexed';
export interface Diagnostic { code: DiagnosticCode; detail?: string }
export interface EffectiveEntry {
  repo: string; source: ConfigurationSource; blocks: Record<string, unknown>; ignored: string[]; coverage: Coverage; diagnostics: Diagnostic[];
}
export interface EffectiveView {
  workspace: string | null; generation: string | null; entries: EffectiveEntry[];
  version?: string; current?: boolean; base?: number; state?: WorkspaceState; hidden?: number;
  recorded?: boolean;
}
export interface ConfigurationFinding { repo: string; code: DiagnosticCode; detail?: string }
export interface Written { base: number; version: string; state?: WorkspaceState; diagnostics?: ConfigurationFinding[] }

export interface VersionRow { version: string; saved: string; message: string; current: boolean; author?: string | null }
export interface VersionList { base: number; versions: VersionRow[]; next: string | null }
export interface VersionDocument {
  id: string; saved: string; message: string; current: boolean; base: number; author?: string | null; hidden?: number;
  entries: { repo: string; blocks: Record<string, unknown> }[]; text: string;
  diff?: { against: string; added: string[]; removed: string[]; changed: { repo: string; keys: string[] }[] };
}
export interface HistoryRow { seq: number; action: string; version: string | null; actor: string | null; authority: string | null; time: string; message: string | null }
export interface HistoryList { base: number; history: HistoryRow[]; next: number | null }

export function versionsRoute(id: string, after?: string): string {
  return after ? `workspaces/${id}/versions?${new URLSearchParams({ after })}` : `workspaces/${id}/versions`;
}
export function versionRoute(id: string, version: string, against = true): string {
  return against ? `workspaces/${id}/versions/${version}?against=current` : `workspaces/${id}/versions/${version}`;
}
export function historyRoute(id: string, after?: number): string {
  return after === undefined ? `workspaces/${id}/history` : `workspaces/${id}/history?after=${after}`;
}

export function effectiveRoute(id: string, version?: string): string {
  return version ? `workspaces/${id}/effective?${new URLSearchParams({ version })}` : `workspaces/${id}/effective`;
}
export function previewBody(entries: string): string { return JSON.stringify({ entries }); }
export function saveBody(entries: string, message: string, base: number): string { return JSON.stringify({ entries, message, base }); }
export function applyBody(version: string, base: number): string { return JSON.stringify({ version, base }); }

export function sourceText(source: ConfigurationSource, t: NamedCopy): string { return t[`source_${source}`]; }
export function coverageText(coverage: Coverage, t: NamedCopy): string { return t[`coverage_${coverage}`]; }
export function diagnosticText(diagnostic: Diagnostic, t: NamedCopy): string {
  const word = t[`diagnostic_${diagnostic.code}`];
  return diagnostic.detail ? `${word} (${diagnostic.detail})` : word;
}
export function findingText(finding: ConfigurationFinding, t: NamedCopy): string {
  return `${finding.repo}: ${diagnosticText(finding, t)}`;
}
