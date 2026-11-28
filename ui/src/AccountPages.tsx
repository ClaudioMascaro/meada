import { useId, useState } from 'react';
import type { FormEvent } from 'react';
import { accountCopy } from './account-i18n';
import { changeCheck, changeProblem, githubProblem, signInProblem, tokenProblem } from './account';
import type { AgentToken } from './account';
import type { Api, Session } from './api';
import { copy } from './i18n';
import type { Language } from './i18n';
import { BREAK_GLASS_PATH } from './local';
import type { LocalAuth } from './local';
import { sessionModel } from './models';
import './account.css';

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

export function ForcedChangePage({ auth, locale }: { auth: LocalAuth; locale: Language }) {
  const t = accountCopy[locale];
  return <PasswordForm auth={auth} locale={locale} title={t.forcedTitle} level={1} intro={t.forcedIntro} />;
}

export function AgentTokenForm({ api, locale, proposals = false }: { api: Api; locale: Language; proposals?: boolean }) {
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
    <select id={surface} name="surface" defaultValue="nodes">
      <option value="nodes">{t.surfaceNodes}</option>
      {proposals && <option value="proposals">{t.surfaceProposals}</option>}
    </select>
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

export function GithubLoginForm({ api, auth, locale, session, onSession }: {
  api: Api; auth: LocalAuth; locale: Language; session: Session; onSession: (session: Session) => void;
}) {
  const t = accountCopy[locale];
  const token = useId();
  const hint = useId();
  const [pending, setPending] = useState<'verify' | 'remove'>();
  const busy = pending !== undefined;
  const [problem, setProblem] = useState<string>();
  const [done, setDone] = useState<string>();

  async function run(pasted: string | undefined, message: string) {
    setPending(pasted === undefined ? 'remove' : 'verify'); setProblem(undefined); setDone(undefined);
    try {
      await auth.github(pasted);
      onSession(await api.session());
      setDone(message);
    } catch (error) { setProblem(githubProblem(error, t)); }
    setPending(undefined);
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const pasted = String(new FormData(form).get('token') ?? '');
    form.reset();
    await run(pasted, t.githubVerified);
  }

  return <form className="account-form" onSubmit={event => { void submit(event); }} aria-busy={busy}>
    <h2>{t.githubTitle}</h2><p className="intro">{t.githubIntro}</p>
    <p role="status">{session.login ? `${t.githubVerifiedAs} ${session.login}` : t.githubNone}</p>
    <label htmlFor={token}>{t.githubToken}</label>
    <input id={token} name="token" type="password" autoComplete="off" spellCheck={false} aria-describedby={hint} required />
    <p id={hint} className="hint">{t.githubTokenHint}</p>
    {problem && <p className="error" role="alert">{problem}</p>}
    {done && <p className="notice" role="status">{done}</p>}
    <button type="submit" disabled={busy}>{pending === 'verify' ? t.githubVerifying : t.githubVerify}</button>
    {session.login && <button type="button" className="secondary" disabled={busy}
      onClick={() => { void run(undefined, t.githubRemoved); }}>{pending === 'remove' ? t.githubRemoving : t.githubRemove}</button>}
  </form>;
}

export function ProfilePage({ api, auth, locale, session, onSession }: {
  api: Api; auth: LocalAuth; locale: Language; session: Session; onSession: (session: Session) => void;
}) {
  const t = accountCopy[locale];
  const common = copy[locale];
  return <>
    <h1>{t.profileTitle}</h1><p className="intro">{t.profileIntro}</p>
    <dl className="fields">{sessionModel(session, common).map(field => <div key={field.label}>
      <dt>{field.label}</dt><dd>{field.value}</dd>
    </div>)}</dl>
    <PasswordForm auth={auth} locale={locale} title={t.changeTitle} level={2} />
    {auth.config.break_glass !== true && <AgentTokenForm api={api} locale={locale} proposals={auth.config.proposals === true} />}
    {auth.config.break_glass !== true && session.github_host !== undefined &&
      <GithubLoginForm api={api} auth={auth} locale={locale} session={session} onSession={onSession} />}
    <button type="button" className="secondary sign-out" onClick={() => auth.clear()}>{common.signOut}</button>
  </>;
}
