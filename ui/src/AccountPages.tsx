import { useId, useState } from 'react';
import type { FormEvent } from 'react';
import { accountCopy } from './account-i18n';
import { changeCheck, changeProblem, signInProblem } from './account';
import type { Session } from './api';
import { copy } from './i18n';
import type { Language } from './i18n';
import type { LocalAuth } from './local';
import { sessionModel } from './models';
import './account.css';

/** The console's own sign-in page: two labelled fields, one generic refusal. */
export function SignInPage({ auth, locale, expired }: { auth: LocalAuth; locale: Language; expired: boolean }) {
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
    <h1>{t.signInTitle}</h1><p className="intro">{t.signInIntro}</p>
    {expired && !problem && <p className="notice" role="status">{t.sessionEnded}</p>}
    <label htmlFor={username}>{t.username}</label>
    <input id={username} name="username" type="text" autoComplete="username" autoCapitalize="none" spellCheck={false} required autoFocus />
    <label htmlFor={password}>{t.password}</label>
    <input id={password} name="password" type="password" autoComplete="current-password" required />
    {problem && <p className="error" role="alert">{problem}</p>}
    <button type="submit" disabled={busy}>{busy ? t.signingIn : t.signInTitle}</button>
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

/** The profile: who the session is, the voluntary password change and sign-out. */
export function ProfilePage({ auth, locale, session }: { auth: LocalAuth; locale: Language; session: Session }) {
  const t = accountCopy[locale];
  const common = copy[locale];
  return <>
    <h1>{t.profileTitle}</h1><p className="intro">{t.profileIntro}</p>
    <dl className="fields">{sessionModel(session, common).map(field => <div key={field.label}>
      <dt>{field.label}</dt><dd>{field.value}</dd>
    </div>)}</dl>
    <PasswordForm auth={auth} locale={locale} title={t.changeTitle} level={2} />
    <button type="button" className="secondary sign-out" onClick={() => auth.clear()}>{common.signOut}</button>
  </>;
}
