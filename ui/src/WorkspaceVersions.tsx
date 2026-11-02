import { useEffect, useRef, useState } from 'react';
import { ApiError, messageOf } from './api';
import type { Api } from './api';
import { ConfirmDialog } from './ConfirmDialog';
import type { Outcome } from './EditWorkspace';
import { diffLines } from './guided';
import { guidedCopy } from './guided-i18n';
import { copy } from './i18n';
import type { Language } from './i18n';
import { applyBody, findingText, historyRoute, versionRoute, versionsRoute } from './named';
import type { HistoryList, HistoryRow, Refusal, VersionDocument, VersionList, VersionRow, WorkspaceDetail, Written } from './named';
import { namedCopy } from './named-i18n';

/**
 * The versions of a named workspace (#503): the list, one version's entries and its diff against the
 * current one, the history for an operator, and applying a listed version. Everything shown is what
 * the routes answer; the page decides nothing, and an apply is sent at the base the page read, which
 * the server checks again at the transaction.
 */
export function WorkspaceVersions({ api, locale, detail, onApplied, onStale }: {
  api: Api; locale: Language; detail: WorkspaceDetail; onApplied: (outcome: Outcome) => void; onStale: () => void;
}) {
  const t = namedCopy[locale];
  const g = guidedCopy[locale];
  const common = copy[locale];
  const [rows, setRows] = useState<VersionRow[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [after, setAfter] = useState<string>();
  const [attempt, setAttempt] = useState(0);
  const [loaded, setLoaded] = useState(false);
  const [shown, setShown] = useState<VersionDocument>();
  const [history, setHistory] = useState<HistoryRow[]>();
  const [historyNext, setHistoryNext] = useState<number | null>(null);
  const [problem, setProblem] = useState<unknown>();
  const [confirming, setConfirming] = useState<VersionDocument>();
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string>();
  const [refusal, setRefusal] = useState<Refusal>();
  const id = detail.id;
  // Only the newest `show` answers: a slower, older read never replaces it.
  const latest = useRef(0);
  useEffect(() => {
    const controller = new AbortController();
    api.request<VersionList>(versionsRoute(id, after), { signal: controller.signal })
      .then(value => {
        if (controller.signal.aborted) return;
        setRows(previous => after === undefined ? value.versions : [...previous, ...value.versions]);
        setNext(value.next); setLoaded(true);
      })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, id, after, attempt, detail.base]);
  // The workspace moved (the editor saved, or this page applied): what was read is of the old base.
  useEffect(() => { latest.current += 1; setAfter(undefined); setShown(undefined); setHistory(undefined); setHistoryNext(null); }, [detail.base]);

  function reload() { latest.current += 1; setAfter(undefined); setAttempt(value => value + 1); setShown(undefined); setHistory(undefined); setHistoryNext(null); setProblem(undefined); }
  const fail = (error: unknown) => setProblem(error);
  async function show(version: string) {
    setProblem(undefined); setFailure(undefined); setRefusal(undefined);
    const mine = ++latest.current;
    try {
      const document = await api.request<VersionDocument>(versionRoute(id, version)).catch(error => {
        if (error instanceof ApiError && error.status === 404) return api.request<VersionDocument>(versionRoute(id, version, false));
        throw error;
      });
      if (mine === latest.current) setShown(document);
    } catch (error) { if (mine === latest.current) fail(error); }
  }
  async function readHistory(from?: number) {
    setProblem(undefined);
    // A read that the base moved past (or a reload) is dropped, like a late `show`.
    const mine = latest.current;
    try {
      const value = await api.request<HistoryList>(historyRoute(id, from));
      if (mine !== latest.current) return;
      setHistory(previous => from === undefined ? value.history : [...(previous ?? []), ...value.history]);
      setHistoryNext(value.next);
    } catch (error) { if (mine === latest.current) fail(error); }
  }
  async function apply(version: VersionDocument) {
    setBusy(true); setFailure(undefined);
    try {
      const applied = await api.request<Written>(`workspaces/${id}/apply`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: applyBody(version.id, version.base) });
      setConfirming(undefined); setFailure(undefined); setRefusal(undefined);
      onApplied({ findings: (applied.diagnostics ?? []).map(finding => findingText(finding, t)) });
      reload();
    } catch (error) {
      setConfirming(undefined);
      if (error instanceof ApiError && error.code === 'entries_refused') {
        setRefusal({ refused: (error.body.refused as { identity: string }[] | undefined) ?? [], hidden: Number(error.body.hidden ?? 0) });
        setFailure(error.message); reload();
      } else if (error instanceof ApiError && (error.code === 'stale_base' || error.code === 'not_operator')) {
        setFailure(error.message); reload(); onStale();
      } else setFailure(messageOf(error, common.loadError));
    } finally { setBusy(false); }
  }

  const diff = shown?.diff;
  const lines = diff ? diffLines(diff, g) : [];
  return <section aria-label={t.versionsTitle}>
    <h3>{t.versionsTitle}</h3>
    {problem ? <p className="error" role="alert">{messageOf(problem, common.loadError)}</p> : null}
    {!loaded && !problem ? <p role="status">{common.loading}</p> : null}
    {loaded && (rows.length === 0 ? <p className="notice">{t.versionsEmpty}</p> : <ul aria-label={t.versionsTitle}>
      {rows.map(row => <li key={row.version}>
        <code>{row.version}</code>{row.current && <> <strong>({t.currentVersion})</strong></>} · {t.savedAt}: {row.saved} · {row.message}
        {row.author != null && <> · {t.author}: {row.author}</>}
        {' '}<button type="button" className="secondary" aria-label={`${t.showVersion} ${row.version}`} onClick={() => { void show(row.version); }}>{t.showVersion}</button>
      </li>)}
    </ul>)}
    {next !== null && <button type="button" className="secondary" onClick={() => setAfter(next)}>{t.moreVersions}</button>}
    {shown && <section aria-label={`${t.versionOf} ${shown.id}`}>
      <h4>{t.versionOf} <code>{shown.id}</code></h4>
      <h5>{t.versionEntries}</h5>
      <ul aria-label={t.versionEntries}>{shown.entries.map(entry => <li key={entry.repo}>{entry.repo}</li>)}</ul>
      {shown.hidden ? <p className="notice">{t.hidden(shown.hidden)}</p> : null}
      <label>{t.manifestOfVersion}<textarea readOnly value={shown.text} rows={8} spellCheck={false} /></label>
      {diff && <section aria-label={t.versionDiff}>
        <h5>{t.versionDiff}</h5>
        {lines.length > 0 ? <ul aria-label={t.versionDiff}>{lines.map(line => <li key={line}>{line}</li>)}</ul> : <p className="notice">{t.noDiff}</p>}
      </section>}
      {detail.operate && !shown.current && <button type="button" onClick={() => setConfirming(shown)}>{t.applyVersion}</button>}
    </section>}
    {refusal && <section role="alert" aria-label={g.refusedSave}>
      <p className="error">{g.refusedSave}</p>
      <ul>{refusal.refused.map(r => <li key={r.identity}>{r.identity}</li>)}</ul>
      {refusal.hidden > 0 && <p>{g.refusedHidden(refusal.hidden)}</p>}
    </section>}
    {failure && <p className="error" role="alert">{failure}</p>}
    {failure && <button type="button" className="secondary" onClick={reload}>{t.reloadVersions}</button>}
    {detail.operate && <section aria-label={t.history}>
      <h4>{t.history}</h4>
      {history === undefined ? <button type="button" className="secondary" onClick={() => { void readHistory(); }}>{t.showHistory}</button>
        : history.length === 0 ? <p className="notice">{t.noHistory}</p> : <table aria-label={t.history}>
          <thead><tr>{[t.seq, t.action, t.versionOf, t.actor, t.authority, t.time, t.message].map(h => <th key={h} scope="col">{h}</th>)}</tr></thead>
          <tbody>{history.map(row => <tr key={row.seq}>
            <td>{row.seq}</td><td>{row.action}</td><td>{row.version ? <code>{row.version}</code> : ''}</td><td>{row.actor ?? ''}</td><td>{row.authority ?? ''}</td><td>{row.time}</td><td>{row.message ?? ''}</td>
          </tr>)}</tbody>
        </table>}
      {historyNext !== null && <button type="button" className="secondary" onClick={() => { void readHistory(historyNext); }}>{t.moreHistory}</button>}
    </section>}
    {confirming && <ConfirmDialog title={t.applyConfirm} note={t.applyConfirmNote} confirmLabel={busy ? t.applying : t.applyVersion} cancelLabel={t.cancel}
      busy={busy} onConfirm={() => { void apply(confirming); }} onCancel={() => setConfirming(undefined)}>
      <p>{t.versionOf}: <code>{confirming.id}</code> · {confirming.message}</p>
      <h3>{t.versionDiff}</h3>
      <ul aria-label={`${t.versionDiff} (${t.applyConfirm})`}>{diffLines(confirming.diff ?? { added: [], removed: [], changed: [] }, g).map(line => <li key={line}>{line}</li>)}</ul>
    </ConfirmDialog>}
  </section>;
}
