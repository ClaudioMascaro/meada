import { useEffect, useId, useState } from 'react';
import type { FormEvent } from 'react';
import { accountsCopy } from './accounts-i18n';
import { accountsModel, changeRoute, createCheck, parseGroups, passwordCheck, refusalText } from './accounts';
import type { AccountRole, Change, ManagedAccount } from './accounts';
import { ApiError } from './api';
import type { Api } from './api';
import { ConfirmDialog } from './ConfirmDialog';
import { copy } from './i18n';
import type { Language } from './i18n';
import './operations.css';
import './account.css';

type Kind = 'role' | 'groups' | 'suspend' | 'reactivate' | 'reset' | 'removeGithub';
interface Target { account: ManagedAccount; kind: Kind }
const post = (body: object): RequestInit => ({ method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });

/** The global operator's accounts page: the list, a create form, and each change behind the shared confirmation dialog. */
export function AccountsPage({ api, locale }: { api: Api; locale: Language }) {
  const t = accountsCopy[locale];
  const common = copy[locale];
  const [accounts, setAccounts] = useState<ManagedAccount[]>();
  const [problem, setProblem] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  const [target, setTarget] = useState<Target>();
  const [done, setDone] = useState<string>();

  useEffect(() => {
    const controller = new AbortController();
    setProblem(undefined);
    api.request<{ accounts: ManagedAccount[] }>('accounts', { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setAccounts(value.accounts); })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, attempt]);

  const reload = (message: string) => { setDone(message); setTarget(undefined); setAttempt(value => value + 1); };
  const rows = accounts ? accountsModel(accounts, t) : [];
  const buttons = (account: ManagedAccount) => {
    const open = (kind: Kind, label: string) => <button type="button" className="secondary" aria-label={`${label}: ${account.username}`} onClick={() => { setDone(undefined); setTarget({ account, kind }); }}>{label}</button>;
    return <div className="account-actions">
      {open('role', t.setRole)}{open('groups', t.setGroups)}
      {account.state === 'suspended' ? open('reactivate', t.reactivate) : open('suspend', t.suspend)}
      {open('reset', t.resetPassword)}
      {account.github_login && open('removeGithub', t.removeGithub)}
    </div>;
  };

  return <>
    <div className="operation-heading"><h1>{t.title}</h1>
      <button type="button" className="secondary" onClick={() => setAttempt(value => value + 1)}>{problem ? common.retry : t.refresh}</button>
    </div>
    <p className="intro">{t.intro}</p>
    {done && <p className="notice" role="status">{done}</p>}
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : common.loadError}</p>
      : !accounts ? <p role="status">{common.loading}</p>
      : <section>
        <h2>{t.list}</h2>
        {rows.length === 0 ? <p className="notice">{t.empty}</p> :
          <div className="table-scroll" role="region" aria-label={t.list} tabIndex={0}>
            <table aria-label={t.list}>
              <thead><tr>{(['username', 'role', 'groups', 'state', 'change', 'changed', 'login', 'actions'] as const).map(column => <th scope="col" key={column}>{t[column]}</th>)}</tr></thead>
              <tbody>{rows.map((row, i) => <tr key={row.username}>
                <th scope="row">{row.username}</th><td>{row.role}</td><td>{row.groups}</td><td>{row.state}</td><td>{row.change}</td><td>{row.changed}</td><td>{row.login}</td>
                <td>{buttons(accounts[i]!)}</td>
              </tr>)}</tbody>
            </table>
          </div>}
      </section>}
    <CreateForm api={api} locale={locale} onCreated={() => reload(t.created)} />
    {target && <ChangeDialog key={`${target.account.username}/${target.kind}`} api={api} locale={locale} target={target}
      onDone={() => reload(t.done)} onCancel={() => setTarget(undefined)} />}
  </>;
}

function CreateForm({ api, locale, onCreated }: { api: Api; locale: Language; onCreated: () => void }) {
  const t = accountsCopy[locale];
  const ids = [useId(), useId(), useId(), useId(), useId(), useId(), useId()] as const;
  const [username, role, groups, groupsHint, password, passwordHint, usernameHint] = ids;
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const form = event.currentTarget;
    const data = new FormData(form);
    const field = (name: string) => String(data.get(name) ?? '');
    const refused = createCheck(field('username'), field('password'), t);
    if (refused) { setProblem(refused); return; }
    setBusy(true); setProblem(undefined);
    try {
      await api.request('accounts', post({
        username: field('username'), role: field('role') === 'operator' ? 'operator' : 'viewer', groups: parseGroups(field('groups')),
        password: field('password'), change_required: data.get('change_required') !== null,
      }));
      form.reset();
      onCreated();
    } catch (error) { setProblem(refusalText(error, t)); }
    setBusy(false);
  }

  return <form className="account-form" onSubmit={event => { void submit(event); }} aria-busy={busy}>
    <h2>{t.createTitle}</h2>
    <label htmlFor={username}>{t.username}</label>
    <input id={username} name="username" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-describedby={usernameHint} required />
    <p id={usernameHint} className="hint">{t.usernameHint}</p>
    <label htmlFor={role}>{t.role}</label>
    <select id={role} name="role" defaultValue="viewer"><option value="viewer">{t.viewer}</option><option value="operator">{t.operator}</option></select>
    <label htmlFor={groups}>{t.groupsLabel}</label>
    <input id={groups} name="groups" type="text" autoComplete="off" autoCapitalize="none" spellCheck={false} aria-describedby={groupsHint} />
    <p id={groupsHint} className="hint">{t.groupsHint}</p>
    <label htmlFor={password}>{t.temporaryPassword}</label>
    <input id={password} name="password" type="password" autoComplete="new-password" aria-describedby={passwordHint} required />
    <p id={passwordHint} className="hint">{t.passwordHint}</p>
    <label className="check"><input name="change_required" type="checkbox" defaultChecked />{t.changeRequired}</label>
    {problem && <p className="error" role="alert">{problem}</p>}
    <button type="submit" disabled={busy}>{busy ? t.creating : t.create}</button>
  </form>;
}

/** One change of one account behind the shared dialog: what it takes (a role, groups, a password) is asked inside it, and a refusal is shown inside it too. */
function ChangeDialog({ api, locale, target, onDone, onCancel }: { api: Api; locale: Language; target: Target; onDone: () => void; onCancel: () => void }) {
  const t = accountsCopy[locale];
  const { account, kind } = target;
  const role = useId();
  const groups = useId();
  const password = useId();
  const hint = useId();
  const [pickedRole, setRole] = useState<AccountRole>(account.role);
  const [groupsText, setGroups] = useState(account.groups.join(', '));
  const [temporary, setTemporary] = useState('');
  const [changeRequired, setChangeRequired] = useState(true);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();

  const title = { role: t.roleTitle, groups: t.groupsTitle, suspend: t.suspendTitle, reactivate: t.reactivateTitle, reset: t.resetTitle, removeGithub: t.removeGithubTitle }[kind];
  const note = { role: t.roleNote, groups: t.groupsNote, suspend: t.suspendNote, reactivate: t.reactivateNote, reset: t.resetNote, removeGithub: t.removeGithubNote }[kind];

  async function confirm() {
    const change: Change = kind === 'role' ? { action: 'role', role: pickedRole }
      : kind === 'groups' ? { action: 'groups', groups: parseGroups(groupsText) }
      : kind === 'suspend' ? { action: 'suspend' } : kind === 'reactivate' ? { action: 'reactivate' }
      : kind === 'removeGithub' ? { action: 'remove_github' }
      : { action: 'reset_password', password: temporary, change_required: changeRequired };
    const refused = change.action === 'reset_password' ? passwordCheck(change.password, t) : undefined;
    if (refused) { setProblem(refused); return; }
    setBusy(true); setProblem(undefined);
    try { await api.request(changeRoute(account.username), post(change)); onDone(); }
    catch (error) { setProblem(refusalText(error, t)); setBusy(false); }
  }

  return <ConfirmDialog title={`${title} ${account.username}`} note={note} confirmLabel={busy ? t.working : t.confirm} cancelLabel={t.cancel}
    busy={busy} error={problem} onConfirm={() => { void confirm(); }} onCancel={onCancel}>
    {kind === 'role' && <div className="account-form">
      <label htmlFor={role}>{t.role}</label>
      <select id={role} value={pickedRole} onChange={event => setRole(event.target.value === 'operator' ? 'operator' : 'viewer')}>
        <option value="viewer">{t.viewer}</option><option value="operator">{t.operator}</option>
      </select>
    </div>}
    {kind === 'groups' && <div className="account-form">
      <label htmlFor={groups}>{t.groupsLabel}</label>
      <input id={groups} type="text" value={groupsText} autoComplete="off" autoCapitalize="none" spellCheck={false} onChange={event => setGroups(event.target.value)} />
    </div>}
    {kind === 'reset' && <div className="account-form">
      <label htmlFor={password}>{t.temporaryPassword}</label>
      <input id={password} type="password" value={temporary} autoComplete="new-password" aria-describedby={hint} onChange={event => setTemporary(event.target.value)} />
      <p id={hint} className="hint">{t.passwordHint}</p>
      <label className="check"><input type="checkbox" checked={changeRequired} onChange={event => setChangeRequired(event.target.checked)} />{t.changeRequired}</label>
    </div>}
  </ConfirmDialog>;
}
