import { useId, useState } from 'react';
import type { FormEvent } from 'react';
import { accountCopy } from './account-i18n';
import { changeCheck, changeProblem, signInProblem, tokenProblem } from './account';
import type { AgentToken } from './account';
import type { Api, Session } from './api';
import { copy } from './i18n';
import type { Language } from './i18n';
import { BREAK_GLASS_PATH } from './local';
import type { LocalAuth } from './local';
import { sessionModel } from './models';
import './account.css';

/**
 * Under break glass (#563), before the identity provider's sign-in: its button first, and a
 * discreet link to the console's own page, which only the default account can pass.
 */
export function SignInChoicePage({ locale, onIdentityProvider }: { locale: Language; onIdentityProvider: () => Promise<void> }) {
  const t = accountCopy[locale];
  const [busy, setBusy] = useState(false);
  const [failed, setFailed] = useState(false);

  async function go() {
    setBusy(true); setFailed(false);
    try { await onIdentityProvider(); } catch { setFailed(true); setBusy(false); }
  }

  return <div className="account-form">
    <h1>{t.chooseTitle}</h1><p className="intro">{t.chooseIntro}</p>
    {failed && <p className="error" role="alert">{t.identityProviderFailed}</p>}
    <button type="button" disabled={busy} onClick={() => { void go(); }}>{busy ? t.redirecting : t.identityProvider}</button>
    <a className="discreet" href={BREAK_GLASS_PATH}>{t.breakGlassLink}</a>
  </div>;
}

/** The console's own sign-in page: two labelled fields, one generic refusal; under break glass, the default account's door with a way back to the identity provider. */
export function SignInPage({ auth, locale, expired, breakGlass = false }: { auth: LocalAuth; locale: Language; expired: boolean; breakGlass?: boolean }) {
  const t = accountCopy[locale];
  const username = useId();
  const password = useId();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = new FormData(event.currentTarget);
    setBusy(true); setProblem(undefined);
    try { await auth.signIn(String(form.get('username') ?? ''), String(form.get('password') ?? '')); }
    catch (error) { setProblem(signInProblem(error, t)); setBusy(false); }
  }

  return <form className="account-form" onSubmit={event => { void submit(event); }} aria-busy={busy}>
    <h1>{breakGlass ? t.breakGlassTitle : t.signInTitle}</h1><p className="intro">{breakGlass ? t.breakGlassIntro : t.signInIntro}</p>
    {expired && !problem && <p className="notice" role="status">{t.sessionEnded}</p>}
    <label htmlFor={username}>{t.username}</label>
    <input id={username} name="username" type="text" autoComplete="username" autoCapitalize="none" spellCheck={false} required autoFocus />
    <label htmlFor={password}>{t.password}</label>
    <input id={password} name="password" type="password" autoComplete="current-password" required />
    {problem && <p className="error" role="alert">{problem}</p>}
    <button type="submit" disabled={busy}>{busy ? t.signingIn : t.signInTitle}</button>
    {breakGlass && <a className="discreet" href="/config">{t.backToIdentityProvider}</a>}
  </form>;
}

/** The password change: the current password and the new one twice, sent to the server once they are filled and match. */
export function PasswordForm({ auth, locale, title, level, intro }: {
  auth: LocalAuth; locale: Language; title: string; level: 1 | 2; intro?: string;
}) {
  const t = accountCopy[locale];
  const current = useId();
  const next = useId();
  const repeat = useId();
  const hint = useId();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();
  const [done, setDone] = useState(false);

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const field = (name: string) => String(data.get(name) ?? '');
    setDone(false);
    const refused = changeCheck(field('current'), field('new'), field('repeat'), t);
    if (refused) { setProblem(refused); return; }
    setBusy(true); setProblem(undefined);
    try {
      await auth.changePassword(field('current'), field('new'));
      // On the forced page the stage flips here and the form unmounts: what follows is for the profile.
      form.reset(); setDone(true);
    } catch (error) { setProblem(changeProblem(error, t)); }
    setBusy(false);
  }

  const Title = level === 1 ? 'h1' : 'h2';
  return <form className="account-form" onSubmit={event => { void submit(event); }} aria-busy={busy}>
    <Title>{title}</Title>
    {intro && <p className="intro">{intro}</p>}
    <label htmlFor={current}>{t.currentPassword}</label>
    <input id={current} name="current" type="password" autoComplete="current-password" required />
    <label htmlFor={next}>{t.newPassword}</label>
    <input id={next} name="new" type="password" autoComplete="new-password" aria-describedby={hint} required />
    <p id={hint} className="hint">{t.changeHint}</p>
    <label htmlFor={repeat}>{t.repeatPassword}</label>
    <input id={repeat} name="repeat" type="password" autoComplete="new-password" required />
    {problem && <p className="error" role="alert">{problem}</p>}
    {done && <p className="notice" role="status">{t.changed}</p>}
    <button type="submit" disabled={busy}>{busy ? t.changing : t.changePassword}</button>
  </form>;
}

/** The forced change: all a change-required session can do, and it offers nothing else. */
export function ForcedChangePage({ auth, locale }: { auth: LocalAuth; locale: Language }) {
  const t = accountCopy[locale];
  return <PasswordForm auth={auth} locale={locale} title={t.forcedTitle} level={1} intro={t.forcedIntro} />;
}

/**
 * An agent token for the nodes (#560): the surface and the validity, then the token shown once with
 * a copy control and its expiry. It lives in this component's state alone, so leaving the page
 * clears it.
 */
export function AgentTokenForm({ api, locale }: { api: Api; locale: Language }) {
  const t = accountCopy[locale];
  const surface = useId();
  const hours = useId();
  const hint = useId();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();
  const [issued, setIssued] = useState<AgentToken>();
  const [copied, setCopied] = useState<string>();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    setBusy(true); setProblem(undefined); setIssued(undefined); setCopied(undefined);
    try {
      const body = JSON.stringify({ surface: String(data.get('surface') ?? 'nodes'), hours: Number(data.get('hours')) });
      setIssued(await api.request<AgentToken>('account/tokens', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body }));
    } catch (error) { setProblem(tokenProblem(error, t)); }
    setBusy(false);
  }

  async function copyToken(token: string) {
    try { await navigator.clipboard.writeText(token); setCopied(t.copied); } catch { setCopied(t.copyFailed); }
  }

  return <form className="account-form" onSubmit={event => { void submit(event); }} aria-busy={busy}>
    <h2>{t.tokensTitle}</h2><p className="intro">{t.tokensIntro}</p>
    <label htmlFor={surface}>{t.surface}</label>
    <select id={surface} name="surface" defaultValue="nodes"><option value="nodes">{t.surfaceNodes}</option></select>
    <label htmlFor={hours}>{t.validity}</label>
    <input id={hours} name="hours" type="number" min={1} max={168} step={1} defaultValue={8} aria-describedby={hint} required />
    <p id={hint} className="hint">{t.validityHint}</p>
    {problem && <p className="error" role="alert">{problem}</p>}
    <button type="submit" disabled={busy}>{busy ? t.issuing : t.issue}</button>
    {issued && <section aria-label={t.issuedToken}>
      <p className="notice" role="status">{t.issuedOnce}</p>
      <output className="token">{issued.token}</output>
      <p>{t.expires} <time dateTime={issued.expires_at}>{issued.expires_at}</time></p>
      <button type="button" className="secondary" onClick={() => { void copyToken(issued.token); }}>{t.copyToken}</button>
      {copied && <p role="status">{copied}</p>}
    </section>}
  </form>;
}

/** The profile: who the session is, the voluntary password change, agent tokens (never under break glass, which has none) and sign-out. */
export function ProfilePage({ api, auth, locale, session }: { api: Api; auth: LocalAuth; locale: Language; session: Session }) {
  const t = accountCopy[locale];
  const common = copy[locale];
  return <>
    <h1>{t.profileTitle}</h1><p className="intro">{t.profileIntro}</p>
    <dl className="fields">{sessionModel(session, common).map(field => <div key={field.label}>
      <dt>{field.label}</dt><dd>{field.value}</dd>
    </div>)}</dl>
    <PasswordForm auth={auth} locale={locale} title={t.changeTitle} level={2} />
    {auth.config.break_glass !== true && <AgentTokenForm api={api} locale={locale} />}
    <button type="button" className="secondary sign-out" onClick={() => auth.clear()}>{common.signOut}</button>
  </>;
}
