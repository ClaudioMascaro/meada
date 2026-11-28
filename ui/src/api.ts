import type { Auth } from './auth';
import type { LocalAuth } from './local';

export interface Config {
  version: string;
  public_url: string;
  operations: boolean;
  audit?: boolean;
  catalog?: boolean;
  workspaces?: { max_entries: number };
  proposals?: boolean;
  sign_in?: 'oidc' | 'accounts';
  break_glass?: boolean;
  issuer?: string;
  client_id?: string;
  scopes?: string[];
  audience?: string;
}

export interface Session {
  subject: string | null; role: 'viewer' | 'team_operator' | 'operator'; groups?: string[]; grant?: '*' | string[]; teams?: string[];
  login?: string; github_host?: string; github_grant?: GithubGrant;
  account?: boolean;
}
export type GithubGrant = 'current' | 'expired' | 'unread';
export interface Module { section: string; feature: string; built: boolean }
export interface Manifest {
  file: string;
  manifest: Record<string, unknown>;
  modules: Module[];
  errors: string[];
}

export class ApiError extends Error {
  constructor(readonly status: number, readonly code: string, message: string, readonly body: Record<string, unknown> = {}) { super(message); }
}

export const messageOf = (error: unknown, fallback: string): string => error instanceof ApiError ? error.message : fallback;

export async function read<T>(response: Response): Promise<T> {
  if (response.ok) {
    return JSON.parse(await response.text(), (_key: string, value: unknown, context?: { source?: string }) => {
      if (typeof value !== 'number' || !Number.isInteger(value) || Number.isSafeInteger(value)) return value;
      if (context?.source === undefined) throw new RangeError('This browser cannot preserve large JSON integers');
      return context.source;
    }) as T;
  }
  const body = await response.json().catch(() => null) as { error?: { code?: string; message?: string } & Record<string, unknown> } | null;
  throw new ApiError(response.status, body?.error?.code ?? 'http', body?.error?.message ?? `HTTP ${response.status}`, body?.error);
}

export function loadConfig(): Promise<Config> {
  return fetch('/api/v1/config', { credentials: 'omit', redirect: 'error', cache: 'no-store' }).then(read<Config>);
}

export function isLocal(auth: Auth | LocalAuth): auth is LocalAuth {
  return 'expire' in auth;
}

async function changeRequired(response: Response): Promise<boolean> {
  const body = await response.clone().json().catch(() => null) as { error?: { code?: unknown } } | null;
  return body?.error?.code === 'password_change_required';
}

export class Api {
  constructor(readonly auth: Auth | LocalAuth) {}

  private async response(route: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (this.auth.token) headers.set('Authorization', `Bearer ${this.auth.token}`);
    const response = await fetch(`/api/v1/${route}`, {
      ...init, headers, credentials: 'omit', redirect: 'error', cache: 'no-store',
    });
    if (response.status === 401 && isLocal(this.auth)) {
      this.auth.expire();
      throw new ApiError(401, 'session_expired', 'session_expired');
    }
    if (response.status === 403 && isLocal(this.auth) && await changeRequired(response)) {
      this.auth.requireChange();
      throw new ApiError(403, 'password_change_required', 'password_change_required');
    }
    if (response.status === 401 && !isLocal(this.auth) && this.auth.config.issuer) {
      this.auth.clear();
      await this.auth.login();
      throw new ApiError(401, 'session_expired', 'session_expired');
    }
    return response;
  }

  async request<T>(route: string, init: RequestInit = {}): Promise<T> {
    return read<T>(await this.response(route, init));
  }

  async export(repository: string, generation: string): Promise<string> {
    const query = generation ? `?${new URLSearchParams({ generation })}` : '';
    const response = await this.response(`repositories/${encodeURIComponent(repository)}/export${query}`);
    if (!response.ok) await read<never>(response);
    return response.text();
  }

  session(): Promise<Session> { return this.request('session'); }
  manifest(): Promise<Manifest> { return this.request('manifest'); }
}
