import { ApiError, read } from './api';
import type { Config } from './api';

export const SESSION_KEY = 'meada.session';

export const BREAK_GLASS_PATH = '/break-glass';

export type Stage = 'out' | 'forced' | 'in';

interface Issued { token: string; expires_at: string; change_required: boolean }

function issued(value: unknown): Issued {
  const body = value as Partial<Issued> | null;
  if (typeof body?.token !== 'string' || !body.token || typeof body.expires_at !== 'string' ||
      typeof body.change_required !== 'boolean' || Number.isNaN(Date.parse(body.expires_at))) throw new Error('login');
  return { token: body.token, expires_at: body.expires_at, change_required: body.change_required };
}

function stored(): Issued | undefined {
  try {
    const text = sessionStorage.getItem(SESSION_KEY);
    if (text === null) return undefined;
    const value = issued(JSON.parse(text));
    if (Date.parse(value.expires_at) <= Date.now()) { sessionStorage.removeItem(SESSION_KEY); return undefined; }
    return value;
  } catch {
    try { sessionStorage.removeItem(SESSION_KEY); } catch {  }
    return undefined;
  }
}

export function breakGlassTab(config: Config, path: string): boolean {
  return config.break_glass === true && (path === BREAK_GLASS_PATH || stored() !== undefined);
}

export class LocalAuth {
  #session?: Issued;
  #expired = false;
  readonly #listeners = new Set<() => void>();

  constructor(readonly config: Config) { this.#session = stored(); }

  get token(): string | undefined { return this.#session?.token; }
  get expired(): boolean { return this.#expired; }
  stage(): Stage { return !this.#session ? 'out' : this.#session.change_required ? 'forced' : 'in'; }

  subscribe = (listener: () => void): (() => void) => {
    this.#listeners.add(listener);
    return () => { this.#listeners.delete(listener); };
  };

  private keep(session: Issued | undefined): void {
    this.#session = session;
    try {
      if (session) sessionStorage.setItem(SESSION_KEY, JSON.stringify(session));
      else sessionStorage.removeItem(SESSION_KEY);
    } catch {  }
    for (const listener of this.#listeners) listener();
  }

  clear(): void { this.#expired = false; this.keep(undefined); }

  expire(): void { this.#expired = true; this.keep(undefined); }

  requireChange(): void {
    if (this.#session) this.keep({ ...this.#session, change_required: true });
  }

  private async post(route: string, body: unknown, bearer?: string): Promise<Issued> {
    const headers = new Headers({ 'Content-Type': 'application/json' });
    if (bearer) headers.set('Authorization', `Bearer ${bearer}`);
    const response = await fetch(`/api/v1/${route}`, {
      method: 'POST', headers, body: JSON.stringify(body), credentials: 'omit', redirect: 'error', cache: 'no-store',
    });
    return issued(await read<unknown>(response));
  }

  async signIn(username: string, password: string): Promise<Stage> {
    const session = await this.post('login', { username, password });
    this.#expired = false;
    this.keep(session);
    return this.stage();
  }

  async changePassword(current: string, next: string): Promise<void> {
    try {
      this.keep(await this.post('account/password', { current, new: next }, this.token));
    } catch (problem) {
      if (problem instanceof ApiError && problem.status === 401 && problem.code !== 'invalid_credentials') this.expire();
      throw problem;
    }
  }

  async github(token?: string): Promise<void> {
    const route = token === undefined ? 'account/github/remove' : 'account/github';
    try {
      this.keep(await this.post(route, token === undefined ? {} : { token }, this.token));
    } catch (problem) {
      if (problem instanceof ApiError && problem.status === 401) this.expire();
      throw problem;
    }
  }
}
