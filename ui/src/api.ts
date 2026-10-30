import type { Auth } from './auth';

export interface Config {
  version: string;
  public_url: string;
  operations: boolean;
  /** With `[postgres]`: the audit page has its route. */
  audit?: boolean;
  /** With `[catalog]` (#341): the catalog and its picker have their routes. */
  catalog?: boolean;
  /** With `[workspaces]` (#342): the named workspaces pages have their routes, and a selection holds at most `max_entries` repositories. */
  workspaces?: { max_entries: number };
  issuer?: string;
  client_id?: string;
  scopes?: string[];
  audience?: string;
}

export interface Session {
  subject: string | null; role: 'viewer' | 'team_operator' | 'operator'; groups?: string[]; grant?: '*' | string[]; teams?: string[];
  /** With `[oidc.github]`: the token's GitHub login (absent without a valid one), the host and the GitHub grant's state. */
  login?: string; github_host?: string; github_grant?: GithubGrant;
}
/** The GitHub grant's state: the permissions set is within `max_age_seconds`, past it, or not read yet. */
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

async function read<T>(response: Response): Promise<T> {
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

/** All operational traffic stays on this console's origin; the bearer exists only in Auth. */
export class Api {
  constructor(readonly auth: Auth) {}

  private async response(route: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    if (this.auth.token) headers.set('Authorization', `Bearer ${this.auth.token}`);
    const response = await fetch(`/api/v1/${route}`, {
      ...init, headers, credentials: 'omit', redirect: 'error', cache: 'no-store',
    });
    if (response.status === 401 && this.auth.config.issuer) {
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
