import { useEffect, useRef, useState, useSyncExternalStore } from 'react';
import type { Config, Manifest, Session } from './api';
import { Api, ApiError, isLocal } from './api';
import { BREAK_GLASS_PATH } from './local';
import type { Stage } from './local';
import { ForcedChangePage, ProfilePage, SignInPage } from './AccountPages';
import { accountCopy } from './account-i18n';
import { copy } from './i18n';
import type { Language } from './i18n';
import { loadCore } from './core';
import { ConfigPage, ManifestPage, SessionPage, Shell, routes } from './pages';
import { PlaygroundPage } from './PlaygroundPage';
import { GraphPage } from './GraphPage';
import { WorkspacePage } from './WorkspacePage';
import { OperationalPage } from './OperationalPage';
import { AuditPage } from './AuditPage';
import { CatalogPage } from './CatalogPage';
import { NamedWorkspacesPage } from './NamedWorkspacesPage';

const noSubscription = () => () => {};

export function App({ api, config, initialLanguage }: { api: Api; config: Config; initialLanguage: Language }) {
  const [locale, setLocale] = useState(initialLanguage);
  const [path, setPath] = useState(location.pathname === '/' ? '/config' : location.pathname);
  const [session, setSession] = useState<Session>();
  const [manifest, setManifest] = useState<Manifest>();
  const [error, setError] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  const [signedOut, setSignedOut] = useState(false);
  const t = copy[locale];
  // With local accounts the tab is signed out, signed in but bound to its password change, or in.
  const local = isLocal(api.auth) ? api.auth : undefined;
  const stage: Stage = useSyncExternalStore(local?.subscribe ?? noSubscription, () => local?.stage() ?? 'in');
  const before = useRef(stage);
  // Under break glass a signed-out tab stays on the console's own page, so a reload keeps it there.
  const signInPath = config.break_glass === true ? BREAK_GLASS_PATH : '/config';
  const operational = (['fleet', 'store', 'repositories', 'credentials', 'admission'] as const).find(kind => path === `/${kind}`);

  function navigate(next: string) {
    setError(undefined);
    history.pushState(null, '', next);
    setPath(next);
    document.getElementById('main')?.focus();
  }

  useEffect(() => {
    const pop = () => { setError(undefined); setPath(location.pathname); };
    addEventListener('popstate', pop);
    return () => removeEventListener('popstate', pop);
  }, []);

  useEffect(() => {
    document.documentElement.lang = locale;
    void loadCore(locale).catch(setError);
  }, [locale]);

  // A session that ends (sign-out or a 401) returns to the first page and forgets what it read; the
  // change that frees a bound session opens the workspace.
  useEffect(() => {
    const was = before.current;
    before.current = stage;
    if (stage === 'out' && was !== 'out') {
      history.replaceState(null, '', signInPath); setPath(signInPath); setSession(undefined); setManifest(undefined); setError(undefined);
    } else if (stage === 'in' && (was === 'forced' || location.pathname === BREAK_GLASS_PATH)) {
      navigate('/workspace');
    }
  }, [stage]);

  useEffect(() => {
    if (signedOut || stage !== 'in') return;
    let current = true;
    setError(undefined);
    api.session().then(value => { if (current) setSession(value); }).catch(problem => { if (current) setError(problem); });
    return () => { current = false; };
  }, [api, attempt, signedOut, stage]);

  useEffect(() => {
    if (path !== '/manifest' || !session || signedOut || stage !== 'in') return;
    let current = true;
    setManifest(undefined);
    setError(undefined);
    api.manifest().then(value => { if (current) setManifest(value); }).catch(problem => { if (current) setError(problem); });
    return () => { current = false; };
  }, [api, path, session, attempt, signedOut, stage]);

  useEffect(() => {
    const route = routes.find(route => route.path === path);
    const name = route ? t[route.label] : path === BREAK_GLASS_PATH ? accountCopy[locale].breakGlassTitle : t.notFound;
    document.title = `${name} · Meada`;
  }, [path, t, locale]);

  async function login() {
    setError(undefined);
    if (isLocal(api.auth)) return;
    try { await api.auth.login(); } catch (problem) { setError(problem); }
  }

  if (local && stage !== 'in') {
    return <Shell path={path} locale={locale} onLanguage={setLocale} t={t} locked>
      {stage === 'forced' ? <ForcedChangePage auth={local} locale={locale} />
        : <SignInPage auth={local} locale={locale} expired={local.expired} breakGlass={config.break_glass === true} />}
    </Shell>;
  }

  const failed = error instanceof ApiError ? error.code === 'session_expired' ? t.sessionExpired : error.message : t.loadError;
  return <Shell path={path} locale={locale} onLanguage={setLocale} session={session} t={t} onNavigate={navigate} audit={config.audit === true} catalog={config.catalog === true} workspaces={config.workspaces !== undefined} profile={local !== undefined}>
    {signedOut ? <>
      <h1>{t.session}</h1><button type="button" onClick={() => { void login(); }}>{t.signIn}</button>
      {error != null && <p className="error" role="alert">{t.loginError}</p>}
    </> : error instanceof ApiError && error.code === 'deprovisioned' ? <>
      <h1>{t.deprovisionedTitle}</h1><p className="error" role="alert">{t.deprovisioned}</p>
      <button type="button" onClick={() => { setError(undefined); setAttempt(value => value + 1); }}>{t.retry}</button>
    </> : error != null ? <>
      <h1>{t.loadError}</h1><p className="error" role="alert">{failed}</p>
      <button type="button" onClick={() => { setError(undefined); setAttempt(value => value + 1); }}>{t.retry}</button>
    </> : path === '/config' ? <ConfigPage config={config} t={t} />
      : path === '/session' ? session ? <SessionPage session={session} t={t} onSignOut={local ? () => local.clear() : config.issuer ? () => {
        api.auth.clear(); setSession(undefined); setSignedOut(true);
      } : undefined} /> : <p role="status">{t.loading}</p>
      : path === '/profile' ? local ? session ? <ProfilePage auth={local} locale={locale} session={session} /> : <p role="status">{t.loading}</p> : <h1>{t.notFound}</h1>
      : path === '/manifest' ? manifest ? <ManifestPage manifest={manifest} t={t} /> : <p role="status">{t.loading}</p>
      : path === '/workspace' ? session ? <WorkspacePage api={api} locale={locale} session={session} config={config} onSession={setSession} /> : <p role="status">{t.loading}</p>
      : path === '/playground' ? <PlaygroundPage api={api} locale={locale} />
      : path === '/graph' ? <GraphPage api={api} locale={locale} />
      : path === '/audit' ? session ? config.audit === true && session.role === 'operator' ? <AuditPage api={api} locale={locale} /> : <h1>{t.notFound}</h1> : <p role="status">{t.loading}</p>
      : path === '/catalog' ? session ? config.catalog === true ? <CatalogPage api={api} locale={locale} /> : <h1>{t.notFound}</h1> : <p role="status">{t.loading}</p>
      : path === '/workspaces' ? session ? config.workspaces ? <NamedWorkspacesPage api={api} locale={locale} session={session} maxEntries={config.workspaces.max_entries} /> : <h1>{t.notFound}</h1> : <p role="status">{t.loading}</p>
      : operational ? session ? <OperationalPage key={operational} kind={operational} api={api} locale={locale} config={config} session={session} /> : <p role="status">{t.loading}</p>
      : <h1>{t.notFound}</h1>}
  </Shell>;
}
