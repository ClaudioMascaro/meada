import type { GuidedCopy } from './guided-i18n';

/**
 * The guided form's view of a named entry (#345): the JSON `namedEntriesJson` gives and
 * `namedEntriesText` takes. The form only reshapes it. Whether a value is valid, what an entry may
 * hold and what the session may select are decided by `meada-core` (WebAssembly) and by the server.
 */
export interface PeerDecl { repo: string; base_url_env?: string[] }
export type ResourceRole = 'read' | 'write' | 'read-write';
export interface ResourceDecl { kind: string; id: string; role: ResourceRole; via?: string[] }
export interface ServeDecl { hosts?: string[]; prefix?: string; strip?: string }
export interface Entry { repo: string; serve?: ServeDecl; resource?: ResourceDecl[]; peer?: PeerDecl[] }

/** The roles a resource declares, as `meada.toml` spells them. */
export const ROLES: readonly ResourceRole[] = ['read', 'write', 'read-write'];

/** The lines of a textarea: trimmed, empty ones dropped. */
export function lines(text: string): string[] {
  return text.split('\n').map(line => line.trim()).filter(line => line !== '');
}

/** An entry without the keys that hold nothing, so a field that is emptied is a field that is absent from the text. */
export function normalize(entry: Entry): Entry {
  const next: Entry = { repo: entry.repo };
  if (entry.serve) {
    const serve: ServeDecl = {};
    if (entry.serve.hosts?.length) serve.hosts = entry.serve.hosts;
    if (entry.serve.prefix) serve.prefix = entry.serve.prefix;
    if (entry.serve.strip) serve.strip = entry.serve.strip;
    next.serve = serve;
  }
  if (entry.resource?.length) {
    next.resource = entry.resource.map(r => ({ kind: r.kind, id: r.id, role: r.role, ...(r.via?.length ? { via: r.via } : {}) }));
  }
  if (entry.peer?.length) {
    next.peer = entry.peer.map(p => ({ repo: p.repo, ...(p.base_url_env?.length ? { base_url_env: p.base_url_env } : {}) }));
  }
  return next;
}

/** The entries as the JSON the core takes. */
export function entriesJson(entries: readonly Entry[]): string {
  return JSON.stringify(entries.map(normalize));
}

export function replaceAt<T>(items: readonly T[], at: number, next: T): T[] {
  return items.map((item, index) => index === at ? next : item);
}

export function removeAt<T>(items: readonly T[], at: number): T[] {
  return items.filter((_, index) => index !== at);
}

/** The change a diff reports for an entry's key, in words. */
function keyText(key: string, t: GuidedCopy): string {
  return key === 'serve' ? t.keyServe : key === 'resource' ? t.keyResource : key === 'peer' ? t.keyPeer : key === 'repository' ? t.keyOrder : key;
}

/** What `namedDiff` computed. */
export interface NamedDiff { added: string[]; removed: string[]; changed: { repo: string; keys: string[] }[] }

/** A diff in words, one line each, in text and never by color alone. */
export function diffLines(diff: NamedDiff, t: GuidedCopy): string[] {
  return [
    ...diff.added.map(repo => `${t.diffAdded}: ${repo}`),
    ...diff.removed.map(repo => `${t.diffRemoved}: ${repo}`),
    ...diff.changed.map(change => `${t.diffChanged}: ${change.repo} (${change.keys.map(key => keyText(key, t)).join(', ')})`),
  ];
}
