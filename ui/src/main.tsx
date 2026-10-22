import { createRoot } from 'react-dom/client';
import { App } from './App';
import { Api, loadConfig } from './api';
import { Auth } from './auth';
import { copy, language } from './i18n';
import { loadCore } from './core';
import './style.css';

const locale = language(navigator.language);
const t = copy[locale];
document.documentElement.lang = locale;
const root = createRoot(document.getElementById('root')!);
root.render(<main><p role="status">{t.loading}</p></main>);

async function start() {
  // Remove the one-time code from the address bar even if discovery or the exchange fails.
  const callback = location.pathname === '/auth/callback' ? new URLSearchParams(location.search) : undefined;
  if (callback) history.replaceState(null, '', '/config');
  const config = await loadConfig();
  if (config.issuer && new URL(config.public_url).origin !== location.origin) {
    sessionStorage.removeItem('meada.pkce.state');
    sessionStorage.removeItem('meada.pkce.verifier');
    location.replace(config.public_url);
    return;
  }
  const auth = new Auth(config);
  if (callback) await auth.callback(callback);
  if (config.issuer && !auth.token) { await auth.login(); return; }
  await loadCore(locale);
  root.render(<App api={new Api(auth)} config={config} initialLanguage={locale} />);
}

void start().catch(error => {
  sessionStorage.removeItem('meada.pkce.state');
  sessionStorage.removeItem('meada.pkce.verifier');
  const message = error instanceof Error && error.message === 'config' ? t.configError : error instanceof Error && error.message === 'login' ? t.loginError : t.loadError;
  root.render(<main><h1>Meada</h1><p className="error" role="alert">{message}</p><a href="/config">{t.retry}</a></main>);
});
