import type { OperationsCopy } from './operations-i18n';
import type { GithubGrant } from './api';

/** A large integer remains decimal text at the HTTP boundary. */
export type Integer = number | string;
export interface Missing { requires: string[] }
export interface Pointer { id: string; fence: Integer }
interface Stuck { kind: string; recoveries: string[] }
export interface Fleet {
  nodes: {
    name: string;
    heartbeat: { generation: string | null; fence: Integer | null; held: string[]; ready: boolean; version: string; time: string };
    workspace: string | null; stuck: Stuck | null;
  }[];
  malformed: { name: string; reason: string }[];
  pointer: Pointer | null;
  retained: (Pointer & { kept: boolean })[];
  holder: { host?: string; pid?: string; started?: string; holds?: string; malformed?: string; ttl_ms: Integer } | null;
  fence: Integer | null; highest_fence: Integer; stuck: Stuck | null;
  last_publish: { generation: string; modified: string | null } | null;
}

export function missingSections(data: Missing, t: OperationsCopy): string {
  return t.requires.replace('{sections}', data.requires.map(section => section === 'github_app' ? '[[github_app]]' : `[${section}]`).join(', '));
}

export function fleetModel(data: Fleet | Missing, t: OperationsCopy) {
  const recoveries = (stuck: Stuck | null) => stuck?.recoveries.map(command => command.startsWith('delete this node') ? t.deletePointer : command) ?? [];
  return {
    missing: 'requires' in data ? missingSections(data, t) : undefined,
    nodes: 'requires' in data ? [] : data.nodes.map(node => ({
      name: node.name, generation: node.heartbeat.generation ?? t.unknown,
      fence: String(node.heartbeat.fence ?? t.unknown), held: node.heartbeat.held.join(', '),
      readiness: node.heartbeat.ready ? t.ready : t.notReady,
      version: node.heartbeat.version, time: node.heartbeat.time,
      workspace: node.workspace ?? t.unknown, recoveries: recoveries(node.stuck),
    })),
    malformed: 'requires' in data ? [] : data.malformed,
    retained: 'requires' in data ? [] : data.retained.map(row => ({ id: row.id, fence: String(row.fence), retention: row.kept ? t.kept : t.retained })),
    recoveries: 'requires' in data ? [] : recoveries(data.stuck),
  };
}

export interface Generation {
  id: string; current: boolean; retained: boolean; kept: boolean; modified?: string | null; malformed?: string;
  document?: { generation: string; workspace?: string; repositories: {
    identity: string; head: string; bundle: string; size: Integer; blake3: string; blocks?: unknown;
  }[] };
}
export interface Store {
  generations: Generation[]; malformed: { key: string | null; reason: string }[];
  pointer: Pointer | null; retained: (Pointer & { kept: boolean })[];
}
export function storeModel(data: Store | Missing, t: OperationsCopy) {
  return {
    missing: 'requires' in data ? missingSections(data, t) : undefined,
    malformed: 'requires' in data ? [] : data.malformed,
    generations: 'requires' in data ? [] : data.generations.map(row => ({
      ...row, loaded: row.document !== undefined,
      retention: [row.current ? t.current : '', row.retained ? t.retained : '', row.kept ? t.kept : ''].filter(Boolean),
      bundles: row.document?.repositories.map(bundle => ({ ...bundle, size: String(bundle.size), hash: bundle.blake3 })) ?? [],
    })),
  };
}

export interface Repositories {
  repositories: ({ identity: string; generation: string; commit: string; schema: Integer | null; nodes: string[] } |
    { identity: string; head: string | null; indexed_commit: string | null; schema: Integer | null; behind: boolean | null } |
    { identity: string; problem: string })[];
}
export function repositoriesModel(data: Repositories | Missing, t: OperationsCopy) {
  return {
    missing: 'requires' in data ? missingSections(data, t) : undefined,
    rows: 'requires' in data ? [] : data.repositories.map(row => ({
      identity: row.identity,
      generation: 'generation' in row ? row.generation : t.local,
      commit: 'commit' in row ? row.commit : 'indexed_commit' in row ? row.indexed_commit ?? t.unknown : t.unknown,
      head: 'head' in row ? row.head ?? t.unknown : t.unknown,
      schema: 'schema' in row ? String(row.schema ?? t.unknown) : t.unknown,
      nodes: 'nodes' in row ? row.nodes.join(', ') : '',
      status: 'problem' in row ? row.problem : 'generation' in row ? t.served : row.behind === true ? t.behind : row.behind === false ? t.upToDate : t.unknown,
    })),
  };
}

export interface Credentials {
  apps: { host: string; app_id: Integer; malformed: boolean; throttled_until: Integer | null;
    installations: { owner: string; id: Integer; cached: boolean; expires: Integer | null;
      remaining_seconds: Integer | null; throttled_until: Integer | null }[] }[];
}
function unixTime(value: Integer | null, t: OperationsCopy): string {
  if (value === null) return t.none;
  const milliseconds = Number(value) * 1000;
  return Number.isSafeInteger(milliseconds) && Math.abs(milliseconds) <= 8.64e15
    ? new Date(milliseconds).toISOString() : String(value);
}
export function credentialsModel(data: Credentials | Missing, t: OperationsCopy) {
  return {
    missing: 'requires' in data ? missingSections(data, t) : undefined,
    apps: 'requires' in data ? [] : data.apps.map(app => ({
      ...app, throttle: unixTime(app.throttled_until, t),
      installations: app.installations.map(installation => ({
        owner: installation.owner, id: String(installation.id),
        cached: installation.cached ? t.cached : t.notCached,
        expires: unixTime(installation.expires, t),
        remaining: installation.remaining_seconds === null ? t.none : `${installation.remaining_seconds} ${t.seconds}`,
        throttle: unixTime(installation.throttled_until, t),
      })),
    })),
  };
}

export interface Admission {
  type: string; issuer: string; metadata: unknown; jwks_uri: string; keys: (string | null)[]; refreshed: string;
  failed: { time: string; reason: string } | null; identities_claim: string; groups_claim: string | null;
  groups: Record<string, string[]> | null; scopes: string[];
  /** With `[directory]`: its counts and the last SCIM write; null when it could not be read. */
  directory?: { users: number; groups: number; last_write: string | null } | null;
  /** With `[oidc.github]`: the provider, the grant's state and the newest permissions set as loaded, its counts the job's. */
  github?: {
    host: string; login_claim: string; roles: string[]; max_age_seconds: number; grant: GithubGrant;
    set: { id: string; finished: string; age_seconds: number; repositories: number; rows: number; exclusions: number } | null;
  };
}
function directoryRows(directory: Admission['directory'], t: OperationsCopy): [string, string][] {
  if (directory === undefined) return [];
  if (directory === null) return [[t.directory, t.directoryUnavailable]];
  return [[t.directoryUsers, String(directory.users)], [t.directoryGroups, String(directory.groups)], [t.directoryLastWrite, directory.last_write ?? t.none]];
}
function githubRows(github: Admission['github'], t: OperationsCopy): [string, string][] {
  if (github === undefined) return [];
  const set = github.set;
  return [
    [t.githubHost, github.host], [t.loginClaim, github.login_claim], [t.githubRoles, github.roles.join(', ')],
    [t.maxAge, `${github.max_age_seconds} ${t.seconds}`], [t.githubGrant, t[`github_${github.grant}`]],
    ...set === null ? [[t.permissionsSet, t.none] as [string, string]] : [
      [t.permissionsSet, set.id], [t.setFinished, set.finished], [t.setAge, `${set.age_seconds} ${t.seconds}`],
      [t.setRepositories, String(set.repositories)], [t.setRows, String(set.rows)], [t.setExclusions, String(set.exclusions)],
    ] as [string, string][],
  ];
}
/** The admission page's fields: the identity provider's type beside the issuer, then the JWKS cache and the claims, the directory and the GitHub grant provider. */
export function admissionModel(data: Admission, t: OperationsCopy): [string, string][] {
  return [
    [t.issuer, data.issuer], [t.providerType, data.type], [t.jwks, data.jwks_uri],
    [t.keyIds, data.keys.map(key => key ?? t.unknown).join(', ') || t.none], [t.refreshed, data.refreshed],
    [t.identitiesClaim, data.identities_claim], [t.groupsClaim, data.groups_claim ?? t.none], [t.scopes, data.scopes.join(', ') || t.none],
    ...directoryRows(data.directory, t),
    ...githubRows(data.github, t),
  ];
}
export interface CheckResult {
  checks: Record<'signature' | 'issuer' | 'audience' | 'lifetime' | 'scopes', boolean | null>;
  identities: string[] | null;
  /** With `[oidc.github]`: the admitted identities by side, each sorted; null when a check failed. */
  sources?: { claims: '*' | string[]; github: string[] } | null;
}
export function checkModel(data: CheckResult, t: OperationsCopy) {
  return {
    checks: (['signature', 'issuer', 'audience', 'lifetime', 'scopes'] as const).map(key => ({
      name: t[key], outcome: data.checks[key] === null ? t.notChecked : data.checks[key] ? t.passed : t.failed,
    })),
    identities: data.identities,
    sources: data.sources && {
      claims: data.sources.claims === '*' ? t.everyIdentity : data.sources.claims.join(', ') || t.none,
      github: data.sources.github.join(', ') || t.none,
    },
  };
}
