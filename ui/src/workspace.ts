import { init, validate, workspaceDiff, workspaceVersionId } from './core';
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

export interface VersionSummary { id: string; author: string; time: string; message: string }
export interface WorkspaceVersion extends VersionSummary { repositories: Record<string, unknown>[] }
export interface WorkspacePointer { id: string; fence: number | string }
export interface CurrentWorkspace { current: WorkspacePointer | null; version: WorkspaceVersion | null }
export interface WorkspaceVersions { current: WorkspacePointer | null; versions: VersionSummary[]; malformed: string[] }

export interface SaveAction {
  kind: 'save'; route: 'workspace/versions'; id: string; command: string;
  body: { entries: string; message: string };
}
export interface ApplyAction {
  kind: 'apply'; route: 'workspace/apply'; rollback: boolean; command: string;
  body: { version: string };
}

export function saveAction(entries: string, message: string, file: string): SaveAction {
  const id = workspaceVersionId(entries);
  if (id === undefined || !message.trim()) throw new TypeError('Invalid workspace save');
  return { kind: 'save', route: 'workspace/versions', id, body: { entries, message },
    command: `meada workspace import <file> --message ${shellQuote(message)} --workspace ${shellQuote(file)}` };
}

export function applyAction(version: VersionSummary, current: string | undefined, versions: VersionSummary[], file: string): ApplyAction {
  const before = versions.findIndex(row => row.id === current);
  const after = versions.findIndex(row => row.id === version.id);
  return { kind: 'apply', route: 'workspace/apply', body: { version: version.id }, rollback: before >= 0 && after > before,
    command: `meada workspace apply ${shellQuote(version.id)} --workspace ${shellQuote(file)}` };
}
