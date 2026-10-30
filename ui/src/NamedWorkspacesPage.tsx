import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError } from './api';
import type { Api, Session } from './api';
import { catalogRoute } from './catalog';
import type { CatalogAnswer } from './catalog';
import { ConfirmDialog } from './ConfirmDialog';
import { loadCore, namedAuthorize, namedEntriesJson, namedEntriesText } from './core';
import { copy } from './i18n';
import type { Language } from './i18n';
import { DESCRIPTION_MAX, MESSAGE_MAX, NAME_MAX, createBody, ingestionText, listRoute, pendingText, plain, stateText } from './named';
import type { Created, Refusal, WorkspaceDetail, WorkspaceList } from './named';
import { namedCopy } from './named-i18n';
import './operations.css';

/** The manifest text of `identities`, as `meada-core` writes it: the one canonical text the server stores. */
function manifestOf(identities: string[]): string {
  return namedEntriesText(JSON.stringify(identities.map(repo => ({ repo }))));
}

/**
 * Named workspaces (#342): the workspaces the session sees, one's detail with the ingestion of each
 * entry in text, and the creation from the repositories the catalog offers. The page decides nothing:
 * `meada-core` (WebAssembly) validates the manifest and marks what the session may not select, and the
 * server decides again at the transaction that creates the workspace.
 */
export function NamedWorkspacesPage({ api, locale, session, maxEntries }: { api: Api; locale: Language; session: Session; maxEntries: number }) {
  const t = namedCopy[locale];
  const common = copy[locale];
  const [list, setList] = useState<WorkspaceList>();
  const [more, setMore] = useState<WorkspaceList['workspaces']>([]);
  const [after, setAfter] = useState<string>();
  const [detail, setDetail] = useState<WorkspaceDetail>();
  const [problem, setProblem] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setProblem(undefined);
    api.request<WorkspaceList>(listRoute(after), { signal: controller.signal })
      .then(value => {
        if (controller.signal.aborted) return;
        setList(value);
        setMore(previous => after === undefined ? value.workspaces : [...previous, ...value.workspaces]);
      })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, after, attempt]);
  function reload() { setAfter(undefined); setList(undefined); setMore([]); setAttempt(value => value + 1); }
  async function show(id: string) {
    setProblem(undefined);
    try { setDetail(await api.request<WorkspaceDetail>(`workspaces/${id}`)); } catch (error) { setProblem(error); }
  }
  return <>
    <div className="operation-heading"><h1>{t.workspaces}</h1>
      <button type="button" className="secondary" onClick={reload}>{problem ? common.retry : t.refresh}</button>
    </div>
    <p className="intro">{t.intro}</p>
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : common.loadError}</p> : null}
    {!list && !problem ? <p role="status">{common.loading}</p> : null}
    {list && <section aria-label={t.list}>
      <h2>{t.list}</h2>
      {more.length === 0 ? <p className="notice">{t.empty}</p> : <ul>
        {more.map(item => <li key={item.id}>
          <strong>{item.name}</strong> <span>{item.description}</span> <span>{stateText(item.state, t)}</span>
          {' '}<button type="button" className="secondary" aria-label={`${t.show} ${item.name}`} onClick={() => { void show(item.id); }}>{t.show}</button>
        </li>)}
      </ul>}
      {list.next !== null && <button type="button" onClick={() => setAfter(list.next ?? undefined)}>{t.more}</button>}
    </section>}
    {detail && <section aria-label={detail.name}>
      <h2>{detail.name}</h2>
      {detail.description && <p>{detail.description}</p>}
      <p role="status">{stateText(detail.state, t)}</p>
      <p>{t.version}: <code>{detail.version}</code> · {t.base}: {detail.base} · {t.created}: {detail.created}</p>
      <p className="notice">{detail.operate ? t.operate : t.viewOnly}</p>
      <h3>{t.entries}</h3>
      <ul aria-label={t.entries}>{detail.entries.map(entry => <li key={entry.repo}>{entry.repo}: {ingestionText(entry.ingestion, t)}</li>)}</ul>
      {detail.hidden ? <p className="notice">{t.hidden(detail.hidden)}</p> : null}
      {detail.operators && <><h3>{t.operators}</h3>
        <ul aria-label={t.operators}>{detail.operators.map(operator => <li key={operator.subject}>{operator.login ?? operator.subject}</li>)}</ul></>}
    </section>}
    <CreateWorkspace api={api} locale={locale} session={session} maxEntries={maxEntries} onCreated={id => { reload(); void show(id); }} />
  </>;
}

/** The creation form: pick from the catalog, name it, review the canonical manifest, confirm. */
function CreateWorkspace({ api, locale, session, maxEntries, onCreated }: { api: Api; locale: Language; session: Session; maxEntries: number; onCreated: (id: string) => void }) {
  const t = namedCopy[locale];
  const [catalog, setCatalog] = useState<CatalogAnswer>();
  const [offered, setOffered] = useState<string[]>([]);
  const [chosen, setChosen] = useState<string[]>([]);
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const [message, setMessage] = useState('');
  const [confirming, setConfirming] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [refusal, setRefusal] = useState<Refusal>();
  const [done, setDone] = useState<string>();
  const [problem, setProblem] = useState<unknown>();
  const [core, setCore] = useState<Language>();
  useEffect(() => {
    let active = true;
    void loadCore(locale).then(() => { if (active) setCore(locale); }).catch(error => { if (active) setProblem(error); });
    return () => { active = false; };
  }, [locale]);
  useEffect(() => {
    const controller = new AbortController();
    api.request<CatalogAnswer>(catalogRoute(), { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) { setCatalog(value); setOffered(value.repositories.map(r => r.identity)); } })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api]);
  function toggle(identity: string) {
    setRefusal(undefined); setDone(undefined);
    setChosen(previous => previous.includes(identity) ? previous.filter(i => i !== identity) : [...previous, identity]);
  }
  // The local rules are the core's, shared with the server: validation of the text, and what the session may select.
  const text = core === locale && chosen.length > 0 ? manifestOf(chosen) : '';
  let invalid: string | undefined;
  if (text) {
    try { namedEntriesJson(text, maxEntries); } catch (error) { invalid = String(error); }
  }
  const grant = session.grant ?? '*';
  const decision = core === locale && chosen.length > 0 ? JSON.parse(namedAuthorize(JSON.stringify(chosen), JSON.stringify(offered), JSON.stringify(grant))) as { allowed: boolean } : { allowed: true };
  const errors: string[] = [];
  if (!plain(name, 1, NAME_MAX)) errors.push(t.invalidName);
  if (!plain(description, 0, DESCRIPTION_MAX)) errors.push(t.invalidDescription);
  if (!plain(message, 1, MESSAGE_MAX)) errors.push(t.invalidMessage);
  if (chosen.length > maxEntries) errors.push(t.tooMany(maxEntries));
  const ready = errors.length === 0 && chosen.length > 0 && invalid === undefined && decision.allowed;
  function review(event: FormEvent) {
    event.preventDefault();
    if (ready) { setFailure(undefined); setConfirming(createBody(name, description, text, message)); }
  }
  async function create(payload: string) {
    setBusy(true); setFailure(undefined);
    try {
      const created = await api.request<Created>('workspaces', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: payload });
      setConfirming(undefined); setDone(created.created ? t.createdNote(name) : t.existing); setChosen([]);
      onCreated(created.id);
    } catch (error) {
      if (error instanceof ApiError && error.code === 'entries_refused') {
        setRefusal({ refused: (error.body.refused as { identity: string }[] | undefined) ?? [], hidden: Number(error.body.hidden ?? 0) });
        setConfirming(undefined);
      } else if (error instanceof ApiError && error.code === 'pending') {
        setFailure(pendingText(error.body.reason as string | undefined, t));
      } else setFailure(error instanceof ApiError ? error.message : copy[locale].loadError);
    } finally { setBusy(false); }
  }
  const pending = catalog?.state === 'pending';
  return <section aria-label={t.new}>
    <h2>{t.new}</h2>
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : copy[locale].loadError}</p> : null}
    {pending && <p role="status">{pendingText(catalog?.reason ?? undefined, t)}</p>}
    <form onSubmit={review}>
      <label>{t.name}<input value={name} maxLength={NAME_MAX * 4} onChange={event => setName(event.target.value)} /></label>
      <label>{t.description}<input value={description} onChange={event => setDescription(event.target.value)} /></label>
      <label>{t.message}<input value={message} onChange={event => setMessage(event.target.value)} /></label>
      <h3>{t.pick}</h3>
      <ul className="catalog-list" aria-label={t.pick}>
        {offered.map(identity => <li key={identity}>
          <label><input type="checkbox" checked={chosen.includes(identity)} aria-label={`${t.select} ${identity}`} onChange={() => toggle(identity)} />{identity}</label>
        </li>)}
      </ul>
      <h3>{t.selected}</h3>
      {chosen.length === 0 ? <p className="notice">{t.noneSelected}</p> : <>
        <ul aria-label={t.selected}>{chosen.map(identity => <li key={identity}>{identity}</li>)}</ul>
        <h3>{t.manifest}</h3><pre aria-label={t.manifest}><code>{text}</code></pre>
      </>}
      {errors.length > 0 && (name || message) ? <ul className="error" role="alert">{errors.map(e => <li key={e}>{e}</li>)}</ul> : null}
      {invalid && <p className="error" role="alert">{invalid}</p>}
      <button type="submit" disabled={!ready}>{t.preview}</button>
    </form>
    {refusal && <section role="alert" aria-label={t.refusedTitle}>
      <p className="error">{t.refusedTitle}</p>
      <ul>{refusal.refused.map(r => <li key={r.identity}>{r.identity}</li>)}</ul>
      {refusal.hidden > 0 && <p>{t.refusedHidden(refusal.hidden)}</p>}
    </section>}
    {done && <p role="status">{done}</p>}
    {confirming !== undefined && <ConfirmDialog title={t.confirmTitle} note={t.confirmNote} confirmLabel={busy ? t.creating : t.create} cancelLabel={t.cancel}
      busy={busy} error={failure} onConfirm={() => { void create(confirming); }} onCancel={() => setConfirming(undefined)}>
      <pre><code>{confirming}</code></pre>
    </ConfirmDialog>}
  </section>;
}
