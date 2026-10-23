import type { Auth } from './auth';

export interface Config {
  version: string;
  public_url: string;
  operations: boolean;
  issuer?: string;
  client_id?: string;
  scopes?: string[];
  audience?: string;
}

export interface Session { subject: string | null; role: 'viewer' | 'operator' }
export interface Module { section: string; feature: string; built: boolean }
export interface Manifest {
  file: string;
  manifest: Record<string, unknown>;
  modules: Module[];
  errors: string[];
}

export class ApiError extends Error {
  constructor(readonly status: number, readonly code: string, message: string) { super(message); }
}

async function read<T>(response: Response): Promise<T> {
  if (response.ok) {
    return JSON.parse(await response.text(), (_key: string, value: unknown, context?: { source?: string }) => {
      if (typeof value !== 'number' || !Number.isInteger(value) || Number.isSafeInteger(value)) return value;
      if (context?.source === undefined) throw new RangeError('This browser cannot preserve large JSON integers');
      return context.source;
    }) as T;
  }
  const body = await response.json().catch(() => null) as { error?: { code?: string; message?: string } } | null;
  throw new ApiError(response.status, body?.error?.code ?? 'http', body?.error?.message ?? `HTTP ${response.status}`);
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
