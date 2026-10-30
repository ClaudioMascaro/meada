import { useEffect, useState } from 'react';
import { ApiError } from './api';
import type { Api, Session } from './api';
import { CHECK_IDENTITIES } from './catalog';
import { ConfirmDialog } from './ConfirmDialog';
import { EffectiveList } from './EffectiveList';
import { loadCore, namedAuthorize, namedChanged, namedDiff, namedEntriesJson, namedEntriesText } from './core';
import { GuidedForm } from './GuidedForm';
import type { EntryStatus } from './GuidedForm';
import { diffLines, entriesJson } from './guided';
import type { Entry, NamedDiff } from './guided';
import { guidedCopy } from './guided-i18n';
import { copy } from './i18n';
import type { Language } from './i18n';
import { MESSAGE_MAX, applyBody, coverageText, diagnosticText, findingText, ingestionText, pendingText, plain, previewBody, saveBody, sourceText } from './named';
import type { EffectiveView, Refusal, WorkspaceDetail, Written } from './named';
import { namedCopy } from './named-i18n';
import type { CatalogView } from './useCatalog';

/** What the person reviewed: nothing else is sent when they confirm. */
interface Frozen { text: string; message: string; save: string; base: number; changes: string[] }

/** The outcome of an applied change, for the page to show once the editor is rebuilt on the new base. */
export interface Outcome { findings: string[] }

/** The entries an operator reads, as the JSON `namedEntriesJson` gives them. */
function entriesOf(detail: WorkspaceDetail): Entry[] {
  return detail.entries.map(entry => ({ repo: entry.repo, ...entry.blocks }) as Entry);
}

/**
 * The guided editor of a workspace the session operates (#345), over the text editor of #344. The
 * form and the text are two views of one document: the canonical text `meada-core` writes. Every
 * check is the core's (WebAssembly) or the server's, which decides again at the transaction, at the
 * base the page read; this component holds the draft, shows the diff and sends only the payload
 * that was reviewed.
 */
export function EditWorkspace({ api, locale, session, detail, savedView, catalog, maxEntries, onChanged, onReload, onLost }: {
  api: Api; locale: Language; session: Session; detail: WorkspaceDetail; savedView?: EffectiveView; catalog: CatalogView; maxEntries: number;
  onChanged: (outcome: Outcome) => void; onReload: () => void; onLost: (notice: string) => void;
}) {
  const t = namedCopy[locale];
  const g = guidedCopy[locale];
  const [core, setCore] = useState<Language>();
  const [text, setText] = useState<string>();
  const [model, setModel] = useState<Entry[]>(() => entriesOf(detail));
  const [formOk, setFormOk] = useState(true);
  const [message, setMessage] = useState('');
  const [view, setView] = useState<EffectiveView>();
  const [frozen, setFrozen] = useState<Frozen>();
  const [busy, setBusy] = useState(false);
  const [checking, setChecking] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [stale, setStale] = useState(false);
  const [storedOnly, setStoredOnly] = useState(false);
  const [refusal, setRefusal] = useState<Refusal>();
  const [problem, setProblem] = useState<unknown>();
  useEffect(() => {
    let active = true;
    void loadCore(locale).then(() => { if (active) setCore(locale); }).catch(error => { if (active) setProblem(error); });
    return () => { active = false; };
  }, [locale]);

  const ready = core === locale;
  const before = entriesOf(detail);
  const original = ready ? namedEntriesText(JSON.stringify(before)) : undefined;
  const current = text ?? original ?? '';
  let next: Entry[] | undefined;
  let invalid: string | undefined;
  if (ready) {
    try { next = JSON.parse(namedEntriesJson(current, maxEntries)) as Entry[]; } catch (error) { invalid = String(error); }
  }
  // The diff, the entries to recheck and the eligibility are the core's: the same functions the server runs.
  const diff = next ? JSON.parse(namedDiff(JSON.stringify(before), JSON.stringify(next))) as NamedDiff : undefined;
  const touched = next ? JSON.parse(namedChanged(JSON.stringify(before), JSON.stringify(next))) as string[] : [];
  const authorization = ready && touched.length > 0
    ? JSON.parse(namedAuthorize(JSON.stringify(touched), JSON.stringify([...catalog.eligible]), JSON.stringify(session.grant ?? '*'))) as { allowed: boolean; named: string[]; hidden: number }
    : { allowed: true, named: [], hidden: 0 };
  // A check that could not decide (`pending`) can be asked again.
  const unverified = touched.filter(identity => !catalog.eligible.has(identity) && (catalog.checked.get(identity) ?? 'pending') === 'pending');

  function edit(value: string) {
    setText(value); setView(undefined); setRefusal(undefined); setStale(false); setStoredOnly(false); setFailure(undefined);
    try { setModel(JSON.parse(namedEntriesJson(value, maxEntries)) as Entry[]); setFormOk(true); } catch { setFormOk(false); }
  }
  function editForm(entries: Entry[]) {
    setModel(entries); setFormOk(true);
    setText(namedEntriesText(entriesJson(entries)));
    setView(undefined); setRefusal(undefined); setStale(false); setStoredOnly(false); setFailure(undefined);
  }
  async function check() {
    setFailure(undefined);
    try {
      setView(await api.request<EffectiveView>('workspaces/effective', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: previewBody(current) }));
    } catch (error) { setFailure(error instanceof ApiError ? error.message : copy[locale].loadError); }
  }
  async function verify() {
    setChecking(true); setFailure(undefined);
    try { await catalog.verify(unverified.slice(0, CHECK_IDENTITIES)); } catch (error) { setFailure(error instanceof ApiError ? error.message : copy[locale].loadError); } finally { setChecking(false); }
  }
  function review() {
    if (!diff) return;
    setFailure(undefined); setStale(false); setStoredOnly(false);
    setFrozen({ text: current, message, save: saveBody(current, message, detail.base), base: detail.base, changes: diffLines(diff, g) });
  }
  async function save(reviewed: Frozen) {
    setBusy(true); setFailure(undefined);
    const headers = { 'Content-Type': 'application/json' };
    let stored = false;
    try {
      const saved = await api.request<Written>(`workspaces/${detail.id}/versions`, { method: 'POST', headers, body: reviewed.save });
      stored = true;
      const applied = await api.request<Written>(`workspaces/${detail.id}/apply`, { method: 'POST', headers, body: applyBody(saved.version, reviewed.base) });
      setFrozen(undefined);
      onChanged({ findings: (applied.diagnostics ?? []).map(finding => findingText(finding, t)) });
    } catch (error) {
      setStoredOnly(stored);
      if (error instanceof ApiError && error.code === 'entries_refused') {
        // The grant changed under the page: the refusal is shown as the server worded it and the roles are read again.
        setRefusal({ refused: (error.body.refused as { identity: string }[] | undefined) ?? [], hidden: Number(error.body.hidden ?? 0) });
        setFrozen(undefined); catalog.reload();
      } else if (error instanceof ApiError && error.code === 'stale_base') {
        setFailure(t.staleBase); setStale(true);
      } else if (error instanceof ApiError && error.code === 'not_operator') {
        setFrozen(undefined); onLost(t.notOperator);
      } else setFailure(error instanceof ApiError ? error.message : copy[locale].loadError);
    } finally { setBusy(false); }
  }

  const changed = original !== undefined && current !== original;
  const ok = ready && invalid === undefined && changed && authorization.allowed && plain(message, 1, MESSAGE_MAX);
  const shown = view ?? savedView;
  const changes = diff ? diffLines(diff, g) : [];
  const status: Record<string, EntryStatus> = {};
  for (const entry of model) {
    const held = detail.entries.find(e => e.repo === entry.repo);
    const effective = shown?.entries.find(e => e.repo === entry.repo);
    const state = catalog.checked.get(entry.repo);
    status[entry.repo] = {
      eligibility: !touched.includes(entry.repo) ? g.unchanged : catalog.eligible.has(entry.repo) ? g.selectable
        : state === 'unavailable' ? g.unavailable : state === 'pending' ? g.pendingVerification : g.notVerified,
      ingestion: held ? ingestionText(held.ingestion, t) : g.newEntry,
      source: effective ? `${sourceText(effective.source, t)} · ${coverageText(effective.coverage, t)} (${view ? g.ofPreview : g.ofSaved})` : undefined,
      diagnostics: (effective?.diagnostics ?? []).map(diagnostic => diagnosticText(diagnostic, t)),
    };
  }
  return <section aria-label={t.edit}>
    <h3>{t.edit}</h3>
    <p className="notice">{t.editNote}</p>
    <section aria-label={g.scope}>
      <h4>{g.scope}</h4>
      <p>{g.scopeOperate(detail.entries.length)}</p>
      {detail.hidden ? <p className="notice">{g.scopeHidden(detail.hidden)}</p> : null}
      {detail.operators && <p>{g.scopeOperators}: {detail.operators.map(operator => operator.login ?? operator.subject).join(', ')}</p>}
    </section>
    {problem || catalog.problem ? <p className="error" role="alert">{(problem ?? catalog.problem) instanceof ApiError ? ((problem ?? catalog.problem) as ApiError).message : copy[locale].loadError}</p> : null}
    {catalog.answer?.state === 'pending' && <p role="status">{pendingText(catalog.answer.reason ?? undefined, t)}</p>}
    <p className="notice">{g.guidedNote}</p>
    {!formOk && <p className="error" role="status">{g.textInvalid}</p>}
    <GuidedForm entries={model} disabled={!ready || !formOk} t={g} status={status} catalog={catalog} onChange={editForm} />
    <label>{t.manifestText}<textarea value={current} rows={12} spellCheck={false} onChange={event => edit(event.target.value)} /></label>
    {invalid && <p className="error" role="alert">{invalid}</p>}
    {!authorization.allowed && <section role="alert" aria-label={g.notEligible}>
      <p className="error">{g.notEligible}</p>
      <ul>{authorization.named.map(identity => <li key={identity}>{identity}</li>)}</ul>
      {authorization.hidden > 0 && <p>{g.notEligibleHidden(authorization.hidden)}</p>}
    </section>}
    <label>{t.message}<input value={message} onChange={event => setMessage(event.target.value)} /></label>
    <button type="button" className="secondary" disabled={!ready || checking || unverified.length === 0} onClick={() => { void verify(); }}>{checking ? g.verifying : g.verify}</button>
    <button type="button" className="secondary" disabled={!ready || invalid !== undefined} onClick={() => { void check(); }}>{t.checkEffective}</button>
    <section aria-label={g.diffTitle}>
      <h4>{g.diffTitle}</h4>
      {changes.length > 0 ? <ul aria-label={g.diffTitle}>{changes.map(line => <li key={line}>{line}</li>)}</ul> : <p className="notice">{g.noChanges}</p>}
    </section>
    {shown && <><EffectiveList view={shown} t={t} title={view ? g.previewOf : g.savedView} /><p className="notice">{t.effectiveNote}</p></>}
    <button type="button" disabled={!ok} onClick={review}>{t.saveApply}</button>
    {refusal && <section role="alert" aria-label={g.refusedSave}>
      <p className="error">{g.refusedSave}</p>
      <ul>{refusal.refused.map(r => <li key={r.identity}>{r.identity}</li>)}</ul>
      {refusal.hidden > 0 && <p>{g.refusedHidden(refusal.hidden)}</p>}
      {storedOnly && <p>{g.storedNotApplied}</p>}
    </section>}
    {frozen && <ConfirmDialog title={g.reviewTitle} note={g.reviewNote} confirmLabel={busy ? t.saving : t.saveApply} cancelLabel={t.cancel}
      busy={busy} error={failure} onConfirm={() => { void save(frozen); }} onCancel={() => setFrozen(undefined)}>
      <h3>{g.diffTitle}</h3>
      <ul aria-label={`${g.diffTitle} (${g.payload})`}>{frozen.changes.map(line => <li key={line}>{line}</li>)}</ul>
      <p>{g.message}: {frozen.message}</p>
      <h3>{g.payload}</h3>
      <pre><code>{frozen.text}</code></pre>
    </ConfirmDialog>}
    {!frozen && failure && <p className="error" role="alert">{failure}</p>}
    {storedOnly && !refusal && <p role="status">{g.storedNotApplied}</p>}
    {stale && <button type="button" className="secondary" onClick={onReload}>{g.reload}</button>}
  </section>;
}
