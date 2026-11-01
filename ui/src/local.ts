import { ApiError, read } from './api';
import type { Config } from './api';

/** The one `sessionStorage` key: the session token and its marks, for the tab's life. */
export const SESSION_KEY = 'meada.session';

/** The console's own sign-in page under break glass (ADR 0024, #563), beside the identity provider's. */
export const BREAK_GLASS_PATH = '/break-glass';

/** Where a tab stands: no session, a session that may only change its password, a full session. */
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
    try { sessionStorage.removeItem(SESSION_KEY); } catch { /* storage may be blocked: the tab simply holds no session */ }
    return undefined;
  }
}

/**
 * Under break glass, whether this tab signs in on the console's own page rather than through the
 * identity provider: it is on that page, or holds a session token from it.
 */
export function breakGlassTab(config: Config, path: string): boolean {
  return config.break_glass === true && (path === BREAK_GLASS_PATH || stored() !== undefined);
}

/**
 * A local account's session (ADR 0024): the session token the console signed, kept in
 * `sessionStorage` (never `localStorage`, a URL or a log) and sent as the bearer. It ends on
 * sign-out, on any 401 and with the tab. No cookie is read or set.
 */
export class LocalAuth {
  #session?: Issued;
  #expired = false;
  readonly #listeners = new Set<() => void>();

  constructor(readonly config: Config) { this.#session = stored(); }

  get token(): string | undefined { return this.#session?.token; }
  /** True after a 401 ended the session, until the next sign-in. */
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
    } catch { /* the token stays in memory for this page load */ }
    for (const listener of this.#listeners) listener();
  }

  /** Sign out: the tab forgets its token. */
  clear(): void { this.#expired = false; this.keep(undefined); }

  /** A 401 ended the session: the tab forgets its token and the sign-in page says so. */
  expire(): void { this.#expired = true; this.keep(undefined); }

  /** The server says this session must change its password first: the tab is bound to the change. */
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

  /** `POST login`: the session token and whether the account must change its password first. */
  async signIn(username: string, password: string): Promise<Stage> {
    const session = await this.post('login', { username, password });
    this.#expired = false;
    this.keep(session);
    return this.stage();
  }

  /**
   * `POST account/password`: a fresh session token without the change-required mark. A 401 that
   * is not a refused current password (`invalid_credentials`) means the session ended.
   */
  async changePassword(current: string, next: string): Promise<void> {
    try {
      this.keep(await this.post('account/password', { current, new: next }, this.token));
    } catch (problem) {
      if (problem instanceof ApiError && problem.status === 401 && problem.code !== 'invalid_credentials') this.expire();
      throw problem;
    }
  }
}
