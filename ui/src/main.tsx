import { createRoot } from 'react-dom/client';
import { App } from './App';
import { Api, loadConfig } from './api';
import { Auth } from './auth';
import { BREAK_GLASS_PATH, LocalAuth, breakGlassTab } from './local';
import { SignInChoicePage } from './AccountPages';
import type { Config } from './api';
import { copy, language } from './i18n';
import { accountCopy } from './account-i18n';
import { loadCore } from './core';
import './style.css';

const locale = language(navigator.language);
const t = copy[locale];
document.documentElement.lang = locale;
const root = createRoot(document.getElementById('root')!);
root.render(<main><p role="status">{t.loading}</p></main>);

let loadedConfig: Config | undefined;

async function start() {
  // Remove the one-time code from the address bar even if discovery or the exchange fails.
  const callback = location.pathname === '/auth/callback' ? new URLSearchParams(location.search) : undefined;
  if (callback) history.replaceState(null, '', '/config');
  const config = loadedConfig = await loadConfig();
  if (config.issuer && new URL(config.public_url).origin !== location.origin) {
    sessionStorage.removeItem('meada.pkce.state');
    sessionStorage.removeItem('meada.pkce.verifier');
    location.replace(config.public_url);
    return;
  }
  // Local accounts (ADR 0024): the console's own sign-in page, the token in this tab's sessionStorage;
  // under break glass (#563), for the tab on that page or holding its session.
  if (config.sign_in === 'accounts' || breakGlassTab(config, location.pathname)) {
    await loadCore(locale);
    root.render(<App api={new Api(new LocalAuth(config))} config={config} initialLanguage={locale} />);
    return;
  }
  const auth = new Auth(config);
  if (callback) await auth.callback(callback);
  if (config.issuer && !auth.token) {
    // Under break glass the identity provider's sign-in waits for a click, beside the discreet door.
    if (config.break_glass === true) { root.render(<main><SignInChoicePage locale={locale} onIdentityProvider={() => auth.login()} /></main>); return; }
    await auth.login(); return;
  }
  await loadCore(locale);
  root.render(<App api={new Api(auth)} config={config} initialLanguage={locale} />);
}

void start().catch(error => {
  sessionStorage.removeItem('meada.pkce.state');
  sessionStorage.removeItem('meada.pkce.verifier');
  const message = error instanceof Error && error.message === 'config' ? t.configError : error instanceof Error && error.message === 'login' ? t.loginError : t.loadError;
  root.render(<main><h1>Meada</h1><p className="error" role="alert">{message}</p><a href="/config">{t.retry}</a>
    {loadedConfig?.break_glass === true && <p><a href={BREAK_GLASS_PATH}>{accountCopy[locale].breakGlassLink}</a></p>}</main>);
});
