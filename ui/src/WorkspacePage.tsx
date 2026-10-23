import { useEffect, useRef, useState } from 'react';
import type { Api, Config, Session } from './api';
import { entriesText, loadCore } from './core';
import type { Language } from './i18n';
import { applyAction, editorModel, saveAction } from './workspace';
import type { ApplyAction, CurrentWorkspace, SaveAction, WorkspaceDiff, WorkspaceVersion, WorkspaceVersions } from './workspace';
import { workspaceCopy } from './workspace-i18n';
import type { WorkspaceCopy } from './workspace-i18n';
import { canOperate } from './actions';
import { ConfirmDialog } from './ConfirmDialog';
import './workspace.css';

export function WorkspacePage({ api, locale, session, config }: { api: Api; locale: Language; config: Config; session: Session }) {
  const t = workspaceCopy[locale];
  const allowed = canOperate(session, config);
  const [ready, setReady] = useState<Language>();
  const [attempt, setAttempt] = useState(0);
  const [workspace, setWorkspace] = useState<CurrentWorkspace>();
  const [listing, setListing] = useState<WorkspaceVersions>();
  const [requires, setRequires] = useState<string[]>([]);
  const [cache, setCache] = useState<Record<string, WorkspaceVersion>>({});
  const [from, setFrom] = useState('');
  const [to, setTo] = useState('');
  const [error, setError] = useState('');
  const [file, setFile] = useState<string>();
  const [draft, setDraft] = useState<string>();
  const [message, setMessage] = useState('');
  const [action, setAction] = useState<SaveAction | ApplyAction>();
  const [busy, setBusy] = useState(false);
  const pending = useRef(false);
  const [operationError, setOperationError] = useState('');
  const [result, setResult] = useState('');
  const [saved, setSaved] = useState<WorkspaceVersion>();
  const [savedId, setSavedId] = useState<string>();
  const [metadataError, setMetadataError] = useState('');

  useEffect(() => {
    let active = true;
    void loadCore(locale).then(() => { if (active) setReady(locale); }).catch(() => { if (active) setError(t.loadError); });
    return () => { active = false; };
  }, [locale, t.loadError]);

  useEffect(() => {
    let active = true;
    setError('');
    void Promise.all([
      api.request<CurrentWorkspace | { requires: string[] }>('workspace'),
      api.request<WorkspaceVersions | { requires: string[] }>('workspace/versions'),
    ]).then(([current, versions]) => {
      if (!active) return;
      if ('requires' in current || 'requires' in versions) {
        setRequires([...new Set([...('requires' in current ? current.requires : []), ...('requires' in versions ? versions.requires : [])])]);
        return;
      }
      setRequires([]); setWorkspace(current); setListing(versions);
      if (current.version) { const version = current.version; setCache(previous => ({ ...previous, [version.id]: version })); }
      const selected = current.current?.id ?? versions.versions[0]?.id ?? '';
      setFrom(previous => previous || selected); setTo(previous => previous || selected);
    }).catch(problem => { if (active) setError(problem instanceof Error ? problem.message : t.loadError); });
    return () => { active = false; };
  }, [api, attempt, t.loadError]);

  useEffect(() => {
    let active = true;
    const ids = [...new Set([from, to])].filter(id => id && !cache[id]);
    if (!ids.length) return;
    void Promise.all(ids.map(id => api.request<WorkspaceVersion>(`workspace/versions/${encodeURIComponent(id)}`))).then(versions => {
      if (active) setCache(previous => ({ ...previous, ...Object.fromEntries(versions.map(version => [version.id, version])) }));
    }).catch(problem => { if (active) setError(problem instanceof Error ? problem.message : t.loadError); });
    return () => { active = false; };
  }, [api, cache, from, to, attempt, t.loadError]);

  useEffect(() => {
    if (!allowed) return;
    let active = true;
    void api.manifest().then(manifest => { if (active) setFile(manifest.file); })
      .catch(problem => { if (active) setError(problem instanceof Error ? problem.message : t.loadError); });
    return () => { active = false; };
  }, [api, allowed, t.loadError]);

  useEffect(() => {
    if (!workspace || ready !== locale) return;
    const initial = workspace.version ? entriesText(JSON.stringify(workspace.version.repositories)) : '';
    setDraft(previous => previous ?? initial);
  }, [workspace, ready, locale]);

  useEffect(() => {
    if (!savedId) return;
    let active = true;
    setMetadataError('');
    void api.request<WorkspaceVersion>(`workspace/versions/${savedId}`).then(version => {
      if (active) { setSaved(version); setCache(previous => ({ ...previous, [version.id]: version })); }
    }).catch(problem => { if (active) setMetadataError(problem instanceof Error ? problem.message : t.loadError); });
    return () => { active = false; };
  }, [api, savedId, attempt, t.loadError]);

  const currentText = workspace?.version && ready === locale ? entriesText(JSON.stringify(workspace.version.repositories)) : '';
  const editor = draft !== undefined && ready === locale ? editorModel(draft, currentText, locale) : undefined;

  async function confirm() {
    if (!action || pending.current) return;
    pending.current = true; setBusy(true); setOperationError('');
    const accepted = action;
    try {
      const answer = await api.request<{ result: string }>(accepted.route, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(accepted.body),
      });
      setResult(answer.result);
      if (accepted.kind === 'save') { setSaved(undefined); setSavedId(accepted.id); }
      setAction(undefined); setAttempt(value => value + 1);
    } catch (problem) { setOperationError(problem instanceof Error ? problem.message : String(problem)); }
    finally { pending.current = false; setBusy(false); }
  }

  const before = cache[from]; const after = cache[to];
  const text = (version: WorkspaceVersion) => entriesText(JSON.stringify(version.repositories));
  const diff = ready === locale && before && after ? editorModel(text(after), text(before), locale).diff : undefined;
  return <>
    <div className="workspace-heading"><div><h1>{t.title}</h1><p className="intro">{t.intro}</p></div>
      <button type="button" className="secondary" onClick={() => setAttempt(value => value + 1)}>{t.refresh}</button></div>
    {result && <p role="status" className="notice">{t.result}: <code>{result}</code></p>}
    {savedId && !saved && !metadataError && <p role="status">{t.savedLoading}</p>}
    {metadataError && <p role="alert" className="error">{metadataError}</p>}
    {saved && <section aria-label={t.saved}><h2>{t.saved}</h2><p><code>{saved.id}</code> · {saved.author} · <time dateTime={saved.time}>{saved.time}</time></p><p>{saved.message}</p></section>}
    {error && <p role="alert" className="error">{error}</p>}
    {requires.length > 0 ? <p className="notice">{t.requires}: {requires.map(name => `[${name}]`).join(', ')}</p> : !listing || !workspace || ready !== locale ? <p role="status">{t.loading}</p> : <>
      {workspace.current && <p>{t.current}: <code>{workspace.current.id}</code> · {t.fence}: <code>{workspace.current.fence}</code></p>}
      <section><h2 id="workspace-versions">{t.versions}</h2>
        {listing.versions.length === 0 ? <p className="notice">{t.empty}</p> : <div className="table-scroll" role="region" aria-label={t.versions} tabIndex={0}>
          <table className="workspace-versions-table"><thead><tr><th>{t.id}</th><th>{t.author}</th><th>{t.time}</th><th>{t.message}</th>{allowed && <th>{t.actions}</th>}</tr></thead><tbody>
            {listing.versions.map(version => {
              const operation = allowed ? applyAction(version, workspace.current?.id, listing.versions, file ?? '') : undefined;
              return <tr key={version.id}><td><code>{version.id}</code>{workspace.current?.id === version.id && <span className="workspace-current">{t.current}</span>}</td>
                <td>{version.author}</td><td><time dateTime={version.time}>{version.time}</time></td><td>{version.message}</td>
                {operation && <td className="workspace-apply"><button type="button" className="secondary" disabled={!file || busy}
                  aria-label={`${operation.rollback ? t.rollback : t.apply} ${version.id}`} onClick={() => { if (file) { setOperationError(''); setAction(operation); } }}>
                  {operation.rollback ? t.rollback : t.apply}</button>{operation.rollback && <p className="notice">{t.rollbackNotice}</p>}</td>}
              </tr>;
            })}
          </tbody></table></div>}
      </section>
      {listing.malformed.length > 0 && <section><h2>{t.malformed}</h2><ul>{listing.malformed.map(name => <li key={name}><code>{name}</code></li>)}</ul></section>}
      {allowed && editor && <form aria-label={t.editor} className="workspace-editor" onSubmit={event => {
        event.preventDefault();
        if (!file || editor.errors.length || !message.trim() || draft === undefined || busy) return;
        setOperationError(''); setAction(saveAction(draft, message, file));
      }}>
        <h2>{t.editor}</h2><p>{t.editorIntro}</p>
        <label className="workspace-label" htmlFor="workspace-edit-entries">{t.editEntries}</label>
        <textarea id="workspace-edit-entries" className="workspace-text" value={draft} spellCheck={false} onChange={event => setDraft(event.target.value)} />
        <div role="region" aria-label={t.validation} tabIndex={0}>{editor.errors.length ? <ul role="alert" className="error">{editor.errors.map((problem, index) => <li key={index}><code>{problem}</code></li>)}</ul> : <p className="notice">{t.valid}</p>}</div>
        {editor.diff && <DiffView diff={editor.diff} t={t} label={t.editorDiff} />}
        <label className="workspace-label" htmlFor="workspace-save-message">{t.saveMessage}</label>
        <input id="workspace-save-message" required value={message} onChange={event => setMessage(event.target.value)} />
        <p>{t.fileHint}</p><button type="submit" disabled={busy || !file || !!editor.errors.length || !message.trim()}>{t.save}</button>
      </form>}
      {listing.versions.length > 0 && <section aria-labelledby="workspace-comparison"><h2 id="workspace-comparison">{t.comparison}</h2>
        <div className="workspace-selectors">{([['from', t.from, from, setFrom], ['to', t.to, to, setTo]] as const).map(([name, label, value, change]) => <label key={name} htmlFor={`workspace-${name}`}>{label}
          <select id={`workspace-${name}`} value={value} onChange={event => change(event.target.value)}>{listing.versions.map(version => <option key={version.id} value={version.id}>{version.id} · {version.message}</option>)}</select></label>)}</div>
        {diff ? <DiffView diff={diff} t={t} /> : <p role="status">{t.loading}</p>}
        <label className="workspace-label" htmlFor="workspace-version-entries">{t.entries}</label>
        <textarea id="workspace-version-entries" className="workspace-text" readOnly value={after && ready === locale ? text(after) : ''} spellCheck={false} />
      </section>}
    </>}
    {allowed && action && <ConfirmDialog title={action.kind === 'save' ? t.save : `${action.rollback ? t.rollbackTitle : t.apply}: ${action.body.version}`} command={action.command}
      confirmLabel={action.kind === 'save' ? t.confirmSave : action.rollback ? t.confirmRollback : t.confirmApply} cancelLabel={t.cancel}
      busy={busy} error={operationError || undefined} onConfirm={() => { void confirm(); }} onCancel={() => setAction(undefined)} />}
  </>;
}

function DiffView({ diff, t, label = t.diff }: { diff: WorkspaceDiff; t: WorkspaceCopy; label?: string }) {
  return <div className="workspace-diff" role="region" aria-label={label} tabIndex={0}>
    <h3>{label}</h3>
    {!diff.added.length && !diff.removed.length && !diff.changed.length ? <p className="notice">{t.noChanges}</p> : <>
      {([['added', t.added], ['removed', t.removed]] as const).map(([key, label]) => diff[key].length > 0 && <div key={key}><h4>{label}</h4><ul>{diff[key].map(repo => <li key={repo}><code>{repo}</code></li>)}</ul></div>)}
      {diff.changed.length > 0 && <div><h4>{t.changed}</h4><table><thead><tr><th>{t.entry}</th><th>{t.key}</th></tr></thead><tbody>{diff.changed.map(change => <tr key={change.repo}><td><code>{change.repo}</code></td><td>{change.keys.map(key => <div key={key}><code>{key}</code>{key === 'repository' && <span> · {t.order}</span>}</div>)}</td></tr>)}</tbody></table></div>}
    </>}
  </div>;
}
