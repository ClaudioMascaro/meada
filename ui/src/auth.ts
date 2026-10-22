import type { Config } from './api';

const STATE = 'meada.pkce.state';
const VERIFIER = 'meada.pkce.verifier';

function base64url(bytes: Uint8Array): string {
  return btoa(String.fromCharCode(...bytes)).replaceAll('+', '-').replaceAll('/', '_').replace(/=+$/, '');
}

export async function challenge(verifier: string): Promise<string> {
  return base64url(new Uint8Array(await crypto.subtle.digest('SHA-256', new TextEncoder().encode(verifier))));
}

function random(): string {
  return base64url(crypto.getRandomValues(new Uint8Array(32)));
}

function endpoint(value: unknown, issuer: URL): string {
  if (typeof value !== 'string') throw new Error('config');
  const url = new URL(value);
  if (url.origin !== issuer.origin || url.username || url.password || url.hash) throw new Error('config');
  return url.href;
}

/** A public OIDC client. The sole persistent state is the pending redirect's PKCE pair. */
export class Auth {
  #token?: string;
  #login?: Promise<void>;

  constructor(
    readonly config: Config,
    readonly redirect: (url: string) => void = url => location.assign(url),
  ) {}

  get token(): string | undefined { return this.#token; }
  clear(): void { this.#token = undefined; }

  private async metadata(): Promise<{ authorization: string; token: string }> {
    if (!this.config.issuer || !this.config.client_id) throw new Error('config');
    const issuer = new URL(this.config.issuer);
    const loopback = ['localhost', '127.0.0.1', '[::1]'].includes(issuer.hostname);
    if ((issuer.protocol !== 'https:' && !(loopback && issuer.protocol === 'http:')) || issuer.username || issuer.password) {
      throw new Error('config');
    }
    const response = await fetch(`${this.config.issuer.replace(/\/$/, '')}/.well-known/openid-configuration`, {
      credentials: 'omit', redirect: 'error', cache: 'no-store',
    });
    if (!response.ok) throw new Error('config');
    const metadata = await response.json() as Record<string, unknown>;
    if (metadata.issuer !== this.config.issuer) throw new Error('config');
    if (Array.isArray(metadata.code_challenge_methods_supported) && !metadata.code_challenge_methods_supported.includes('S256')) {
      throw new Error('config');
    }
    return {
      authorization: endpoint(metadata.authorization_endpoint, issuer),
      token: endpoint(metadata.token_endpoint, issuer),
    };
  }

  login(): Promise<void> {
    return this.#login ??= this.startLogin().catch(error => { this.#login = undefined; throw error; });
  }

  private async startLogin(): Promise<void> {
    this.clear();
    const metadata = await this.metadata();
    const state = random();
    const verifier = random();
    const url = new URL(metadata.authorization);
    url.search = new URLSearchParams({
      response_type: 'code', client_id: this.config.client_id!,
      redirect_uri: `${this.config.public_url}/auth/callback`,
      scope: [...new Set(['openid', ...this.config.scopes ?? []])].join(' '),
      state, code_challenge: await challenge(verifier), code_challenge_method: 'S256',
      ...this.config.audience ? { audience: this.config.audience } : {},
    }).toString();
    sessionStorage.setItem(STATE, state);
    sessionStorage.setItem(VERIFIER, verifier);
    this.redirect(url.href);
  }

  async callback(parameters: URLSearchParams): Promise<void> {
    const state = sessionStorage.getItem(STATE);
    const verifier = sessionStorage.getItem(VERIFIER);
    sessionStorage.removeItem(STATE);
    sessionStorage.removeItem(VERIFIER);
    this.clear();
    const code = parameters.get('code');
    if (!state || !verifier || parameters.get('state') !== state || !code || parameters.has('error')) {
      throw new Error('login');
    }
    const metadata = await this.metadata();
    const response = await fetch(metadata.token, {
      method: 'POST', credentials: 'omit', redirect: 'error', cache: 'no-store',
      body: new URLSearchParams({
        grant_type: 'authorization_code', client_id: this.config.client_id!, code,
        redirect_uri: `${this.config.public_url}/auth/callback`, code_verifier: verifier,
      }),
    });
    if (!response.ok) throw new Error('login');
    const token = await response.json() as Record<string, unknown>;
    if (typeof token.access_token !== 'string' || !token.access_token ||
        typeof token.token_type !== 'string' || token.token_type.toLowerCase() !== 'bearer') throw new Error('login');
    this.#token = token.access_token;
    this.#login = undefined;
  }
}
