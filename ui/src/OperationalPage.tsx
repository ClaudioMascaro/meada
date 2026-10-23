import { useEffect, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { ApiError } from './api';
import type { Api } from './api';
import type { Config, Session } from './api';
import { ConfirmDialog } from './ConfirmDialog';
import { ActionControls, RecoveryControl } from './ActionControls';
import { canOperate, pointerActions } from './actions';
import type { Operation } from './actions';
import { actionsCopy } from './actions-i18n';
import type { ActionButtonsProps } from './ActionControls';
import { copy } from './i18n';
import type { Language } from './i18n';
import { checkModel, credentialsModel, fleetModel, missingSections, repositoriesModel, storeModel } from './operations';
import type { Admission, CheckResult, Credentials, Fleet, Generation, Missing, Repositories, Store } from './operations';
import { operationsCopy } from './operations-i18n';
import type { OperationsCopy } from './operations-i18n';
import './operations.css';

export type OperationalKind = 'fleet' | 'store' | 'repositories' | 'credentials' | 'admission';
interface Props { kind: OperationalKind; api: Api; locale: Language; config: Config; session: Session }
interface Access { config: Config; session: Session; file: string }
interface Actions extends ActionButtonsProps { access?: Access }

function isMissing(data: object): data is Missing { return 'requires' in data; }

function Read<T extends object>({ kind, api, locale, children, revision }: Pick<Props, 'kind' | 'api' | 'locale'> & { revision: number; children: (data: T) => ReactNode }) {
  const [data, setData] = useState<T | Missing>();
  const [problem, setProblem] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  const t = operationsCopy[locale];
  const common = copy[locale];
  useEffect(() => {
    const controller = new AbortController();
    setData(undefined); setProblem(undefined);
    api.request<T | Missing>(kind, { signal: controller.signal }).then(value => {
      if (!controller.signal.aborted) setData(value);
    }).catch(error => {
      if (controller.signal.aborted) return;
      if (kind === 'admission' && error instanceof ApiError && error.status === 404) setData({ requires: ['oidc'] });
      else setProblem(error);
    });
    return () => controller.abort();
  }, [kind, api, attempt, revision]);
  return <>
    <div className="operation-heading"><h1>{t[kind]}</h1>
      <button type="button" className="secondary" onClick={() => setAttempt(value => value + 1)}>{problem ? common.retry : t.refresh}</button>
    </div>
    <p className="intro">{t[`${kind}Intro`]}</p>
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : common.loadError}</p>
      : !data ? <p role="status">{common.loading}</p>
      : isMissing(data) ? <p className="notice">{missingSections(data, t)}</p>
      : children(data)}
  </>;
}

function Fields({ rows }: { rows: [string, ReactNode][] }) {
  return <dl className="fields">{rows.map(([name, value]) => <div key={name}><dt>{name}</dt><dd>{value}</dd></div>)}</dl>;
}
function Table({ label, headers, rows, t }: { label: string; headers: string[]; rows: ReactNode[][]; t: OperationsCopy }) {
  return <section><h2>{label}</h2>{rows.length === 0 ? <p className="notice">{t.empty}</p> :
    <div className="table-scroll" role="region" aria-label={label} tabIndex={0}>
      <table aria-label={label}><thead><tr>{headers.map((header, index) => <th scope="col" key={index}>{header}</th>)}</tr></thead>
        <tbody>{rows.map((cells, index) => <tr key={index}>{cells.map((cell, column) => <td key={column}>{cell}</td>)}</tr>)}</tbody>
      </table>
    </div>}</section>;
}
function Commands({ commands }: { commands: string[] }) {
  return commands.length ? <ul className="recovery-commands">{commands.map(command => <li key={command}><code>{command}</code></li>)}</ul> : null;
}

function FleetView({ data, t, actions }: { data: Fleet; t: OperationsCopy; actions: Actions }) {
  const model = fleetModel(data, t);
  const holder = data.holder;
  return <>
    <Fields rows={[
      [t.pointer, data.pointer ? <code>{data.pointer.id} ({t.fence}: {data.pointer.fence})</code> : t.none],
      [t.fence, String(data.fence ?? t.unknown)], [t.highestFence, String(data.highest_fence)],
      [t.lockHolder, holder ? holder.malformed ?? `${holder.host} / PID ${holder.pid} / ${holder.holds}` : t.none],
      [t.started, holder?.started ?? t.none], [t.ttl, String(holder?.ttl_ms ?? t.none)],
      [t.lastPublish, data.last_publish ? <><code>{data.last_publish.generation}</code> / {data.last_publish.modified ?? t.unknown}</> : t.none],
    ]} />
    {model.recoveries.length > 0 && <section className="error"><h2>{t.stuckFleet}</h2><Commands commands={model.recoveries} />
      {actions.access && <RecoveryControl {...actions} file={actions.access.file} identity={t.fleet} />}</section>}
    <Table label={t.nodes} t={t} headers={[t.name, t.generation, t.fence, t.held, t.readiness, t.version, t.time, t.workspace, t.recovery]}
      rows={model.nodes.map(node => [node.name, <code>{node.generation}</code>, node.fence, <code>{node.held}</code>, node.readiness, node.version, node.time, <code>{node.workspace}</code>, <><Commands commands={node.recoveries} />
        {node.recoveries.length > 0 && actions.access && <RecoveryControl {...actions} file={actions.access.file} identity={node.name} />}</>])} />
    <Table label={t.retention} t={t} headers={[t.generation, t.fence, t.retention, ...actions.access ? [actions.t.actions] : []]} rows={model.retained.map(row => {
      const retained = data.retained.find(entry => entry.id === row.id)!;
      const access = actions.access;
      return [<code>{row.id}</code>, row.fence, row.retention, ...access ? [<ActionControls {...actions} identity={row.id}
        operations={pointerActions({ ...retained, retained: true, current: data.pointer?.id === row.id }, access.session, access.config, access.file)} />] : []];
    })} />
    {model.malformed.length > 0 && <Table label={t.malformed} t={t} headers={[t.name, t.reason]} rows={model.malformed.map(row => [row.name, row.reason])} />}
  </>;
}

function Bundles({ row, t }: { row: Generation; t: OperationsCopy }) {
  const model = storeModel({ generations: [row], malformed: [], pointer: null, retained: [] }, t).generations[0];
  return model && <section>
    <h2><code>{row.id}</code></h2>
    <p>{model.retention.join(', ') || t.none}{row.modified ? ` / ${row.modified}` : ''}</p>
    {row.malformed ? <p className="error">{row.malformed}</p> : <>
      <p>{t.workspace}: <code>{row.document?.workspace ?? t.unknown}</code></p>
      <Table label={`${t.bundles}: ${row.id}`} t={t} headers={[t.identity, t.commit, t.bundle, t.size, t.hash]}
        rows={model.bundles.map(bundle => [bundle.identity, <code>{bundle.head}</code>, <code>{bundle.bundle}</code>, bundle.size, <code>{bundle.hash}</code>])} />
    </>}
  </section>;
}

function StoreView({ data, api, t, locale, actions }: { data: Store; api: Api; t: OperationsCopy; locale: Language; actions: Actions }) {
  const [selected, setSelected] = useState<Generation>();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<unknown>();
  const model = storeModel(data, t);
  async function inspect(id: string) {
    setBusy(true); setProblem(undefined); setSelected(undefined);
    try { setSelected(await api.request<Generation>(`store/generations/${encodeURIComponent(id)}`)); }
    catch (error) { setProblem(error); }
    finally { setBusy(false); }
  }
  return <>
    <Fields rows={[
      [t.pointer, data.pointer ? <code>{data.pointer.id} ({t.fence}: {data.pointer.fence})</code> : t.none],
      [t.retention, data.retained.map(row => `${row.id} (${row.fence})`).join(', ') || t.none],
    ]} />
    <Table label={t.generations} t={t} headers={[t.generation, t.retention, t.modified, t.reason, t.inspect, ...actions.access ? [actions.t.actions] : []]}
      rows={model.generations.map(row => [<code>{row.id}</code>, row.retention.join(', ') || t.none, row.modified ?? t.unknown, row.malformed ?? t.none,
        <button type="button" className="secondary" disabled={busy} aria-label={`${t.inspect} ${row.id}`} onClick={() => { void inspect(row.id); }}>{t.inspect}</button>,
        ...actions.access ? [<ActionControls {...actions} identity={row.id} operations={pointerActions(row, actions.access.session, actions.access.config, actions.access.file)} />] : []])} />
    {busy && <p role="status">{copy[locale].loading}</p>}
    {problem != null && <p className="error" role="alert">{problem instanceof ApiError ? problem.message : copy[locale].loadError}</p>}
    {selected && <Bundles row={selected} t={t} />}
    {model.generations.filter(row => row.loaded && row.id !== selected?.id).map(row => <Bundles key={row.id} row={row} t={t} />)}
    {model.malformed.length > 0 && <Table label={t.malformed} t={t} headers={[t.generation, t.reason]} rows={model.malformed.map(row => [row.key ?? t.unknown, row.reason])} />}
  </>;
}

function RepositoriesView({ data, t }: { data: Repositories; t: OperationsCopy }) {
  const model = repositoriesModel(data, t);
  return <Table label={t.repositories} t={t} headers={[t.identity, t.generation, t.commit, 'HEAD', t.schema, t.nodes, t.state]}
    rows={model.rows.map(row => [row.identity, <code>{row.generation}</code>, <code>{row.commit}</code>, <code>{row.head}</code>, row.schema, row.nodes, row.status])} />;
}
function CredentialsView({ data, t }: { data: Credentials; t: OperationsCopy }) {
  const model = credentialsModel(data, t);
  return model.apps.length === 0 ? <p className="notice">{t.empty}</p> : model.apps.map(app => <section key={`${app.host}/${app.app_id}`}>
    <h2>{app.host} / {app.app_id}</h2>
    <p>{t.throttle}: {app.throttle}</p>
    {app.malformed && <p className="error">{t.cacheMalformed}</p>}
    <Table label={`${t.installation}: ${app.host}/${app.app_id}`} t={t}
      headers={[t.owner, t.installation, t.cache, t.expires, t.remaining, t.throttle]}
      rows={app.installations.map(row => [row.owner, row.id, row.cached, row.expires, row.remaining, row.throttle])} />
  </section>);
}

function AdmissionView({ data, api, t, locale }: { data: Admission; api: Api; t: OperationsCopy; locale: Language }) {
  const token = useRef<HTMLTextAreaElement>(null);
  const [result, setResult] = useState<CheckResult>();
  const [problem, setProblem] = useState<unknown>();
  const [busy, setBusy] = useState(false);
  async function check() {
    setBusy(true); setProblem(undefined); setResult(undefined);
    try {
      const value = await api.request<CheckResult>('admission/check', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ token: token.current?.value.trim() ?? '' }),
      });
      setResult(value);
    } catch (error) { setProblem(error); }
    finally { if (token.current) token.current.value = ''; setBusy(false); }
  }
  const checked = result && checkModel(result, t);
  return <>
    <Fields rows={[
      [t.issuer, data.issuer], [t.jwks, data.jwks_uri], [t.keyIds, data.keys.map(key => key ?? t.unknown).join(', ') || t.none],
      [t.refreshed, data.refreshed], [t.identitiesClaim, data.identities_claim], [t.groupsClaim, data.groups_claim ?? t.none], [t.scopes, data.scopes.join(', ') || t.none],
    ]} />
    {data.failed && <p className="error">{data.failed.time}: {data.failed.reason}</p>}
    <Table label={t.groups} t={t} headers={[t.group, t.identities]} rows={Object.entries(data.groups ?? {}).map(([group, identities]) => [group, identities.join(', ') || t.none])} />
    <h2>{t.metadata}</h2><pre className="configuration" role="region" aria-label={t.metadata} tabIndex={0}>{JSON.stringify(data.metadata, null, 2)}</pre>
    <form className="token-check" aria-label={t.checkToken} onSubmit={event => { event.preventDefault(); void check(); }}>
      <label htmlFor="check-token">{t.token}</label>
      <p id="token-hint">{t.tokenHint}</p>
      <textarea id="check-token" ref={token} required autoComplete="off" spellCheck={false} aria-describedby="token-hint" disabled={busy} />
      <button type="submit" disabled={busy}>{busy ? t.checking : t.checkToken}</button>
    </form>
    {problem != null && <p className="error" role="alert">{problem instanceof ApiError ? problem.message : copy[locale].loadError}</p>}
    {checked && <>
      <Table label={t.checkToken} t={t} headers={[t.check, t.outcome]} rows={checked.checks.map(row => [row.name, row.outcome])} />
      <p>{t.identities}: {checked.identities === null ? t.noGrant : checked.identities.join(', ') || t.none}</p>
    </>}
  </>;
}

export function OperationalPage({ kind, api, locale, config, session }: Props) {
  const t = operationsCopy[locale];
  const a = actionsCopy[locale];
  const [file, setFile] = useState<string>();
  const [manifestProblem, setManifestProblem] = useState(false);
  const [pending, setPending] = useState<Operation>();
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string>();
  const [result, setResult] = useState<string>();
  const [reloadProblem, setReloadProblem] = useState(false);
  const [revision, setRevision] = useState(0);
  const eligible = canOperate(session, config) && (kind === 'fleet' || kind === 'store');
  useEffect(() => {
    let current = true;
    setFile(undefined); setManifestProblem(false);
    if (eligible) api.manifest().then(value => { if (current) setFile(value.file); })
      .catch(() => { if (current) setManifestProblem(true); });
    return () => { current = false; };
  }, [api, eligible]);
  async function confirm() {
    if (!pending || busy || !eligible) return;
    setBusy(true); setProblem(undefined);
    let line: string;
    try {
      const response = await api.request<{ result: string }>(pending.route, {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(pending.body),
      });
      line = response.result;
    } catch (error) {
      setProblem(error instanceof ApiError ? error.message : copy[locale].loadError); setBusy(false); return;
    }
    setResult(line); setPending(undefined); setRevision(value => value + 1); setReloadProblem(false);
    if (kind !== 'fleet') {
      try { await api.request<Fleet>('fleet'); } catch { setReloadProblem(true); }
    }
    setBusy(false);
  }
  const actions: Actions = { t: a, busy, onOpen: operation => { setPending(operation); setProblem(undefined); },
    access: eligible && file !== undefined ? { config, session, file } : undefined };
  const props = { kind, api, locale, revision };
  let page: ReactNode;
  switch (kind) {
    case 'fleet': page = <Read<Fleet> key={kind} {...props}>{data => <FleetView data={data} t={t} actions={actions} />}</Read>; break;
    case 'store': page = <Read<Store> key={kind} {...props}>{data => <StoreView data={data} api={api} t={t} locale={locale} actions={actions} />}</Read>; break;
    case 'repositories': page = <Read<Repositories> key={kind} {...props}>{data => <RepositoriesView data={data} t={t} />}</Read>; break;
    case 'credentials': page = <Read<Credentials> key={kind} {...props}>{data => <CredentialsView data={data} t={t} />}</Read>; break;
    case 'admission': page = <Read<Admission> key={kind} {...props}>{data => <AdmissionView data={data} api={api} t={t} locale={locale} />}</Read>; break;
  }
  return <>
    {result && <p className="notice" role="status">{result}</p>}
    {reloadProblem && <p className="error" role="alert">{a.reloadError}</p>}
    {manifestProblem && <p className="error" role="alert">{a.manifestError}</p>}
    {page}
    {pending && eligible && <ConfirmDialog title={a[pending.action]} command={pending.command} confirmLabel={a.confirm} cancelLabel={a.cancel}
      busy={busy} error={problem} onConfirm={() => { void confirm(); }} onCancel={() => setPending(undefined)} />}
  </>;
}
