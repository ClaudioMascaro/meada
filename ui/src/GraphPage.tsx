import { useRef, useState } from 'react';
import type { ReactNode } from 'react';
import type { Api } from './api';
import type { Language } from './i18n';
import { graphCopy } from './graph-i18n';
import { graphModel } from './graph';
import type { GraphModel } from './graph';
import './graph.css';

const views = ['communities', 'contracts', 'links', 'drift', 'decisions'] as const;
type View = typeof views[number];

function List({ values }: { values: string[] }) {
  return <ul className="graph-list">{values.map((value, index) => <li key={index}><code>{value}</code></li>)}</ul>;
}

function GraphTable({ title, headers, rows, empty }: { title: string; headers: string[]; rows: ReactNode[][]; empty: string }) {
  return <section><h2>{title}</h2>
    <div className="table-scroll graph-table" role="region" aria-label={title} tabIndex={0}>
      <table aria-label={title}>
        <thead><tr>{headers.map(header => <th scope="col" key={header}>{header}</th>)}</tr></thead>
        <tbody>{rows.length === 0 ? <tr><td colSpan={headers.length}>{empty}</td></tr> : rows.map((row, index) =>
          <tr key={index}>{row.map((cell, column) => <td key={column}>{cell}</td>)}</tr>)}</tbody>
      </table>
    </div>
  </section>;
}

export function GraphPage({ api, locale }: { api: Api; locale: Language }) {
  const t = graphCopy[locale];
  const [repository, setRepository] = useState('');
  const [generation, setGeneration] = useState('');
  const [view, setView] = useState<View>('communities');
  const [graph, setGraph] = useState<GraphModel>();
  const [loadedGeneration, setLoadedGeneration] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const exports = useRef(new Map<string, GraphModel>());

  async function load() {
    setError(''); setBusy(true); setGraph(undefined);
    try {
      const key = JSON.stringify([repository, generation]);
      let result = generation ? exports.current.get(key) : undefined;
      if (!result) {
        result = graphModel(await api.export(repository, generation));
        if (generation) exports.current.set(key, result);
      }
      setGraph(result); setLoadedGeneration(generation);
    } catch (problem) { setError(problem instanceof Error ? problem.message : typeof problem === 'string' ? problem : t.failed); }
    finally { setBusy(false); }
  }

  function table(title: string, headers: string[], rows: ReactNode[][]) {
    return <GraphTable title={title} headers={headers} rows={rows} empty={t.noResults} />;
  }

  return <>
    <h1>{t.graph}</h1><p className="intro">{t.graphIntro}</p>
    <form className="graph-form" aria-label={t.graph} onSubmit={event => { event.preventDefault(); void load(); }}>
      <label>{t.repository}<input required value={repository} onChange={event => setRepository(event.target.value)} placeholder="example.com/team/repository" autoCapitalize="none" spellCheck={false} /></label>
      <label>{t.generation}<input value={generation} onChange={event => setGeneration(event.target.value)} placeholder={t.current} pattern="[0-9a-f]{16}" autoCapitalize="none" spellCheck={false} /></label>
      <div className="graph-form-actions"><button type="submit" disabled={busy}>{busy ? t.loading : t.load}</button></div>
    </form>
    {error && <p className="error" role="alert">{error}</p>}
    {graph && <>
      <p className="file-path">{t.exportLoaded}: <code>{graph.repository}</code> · {t.generation}: <code>{loadedGeneration || t.current}</code></p>
      <label className="graph-view-picker">{t.view}<select value={view} onChange={event => {
        const next = views.find(view => view === event.target.value);
        if (next) setView(next);
      }}>{views.map(view => <option value={view} key={view}>{t[view]}</option>)}</select></label>
      {view === 'communities' && table(t.communities, [t.community, t.members], graph.communities.map(row => [row.name, <List values={row.members} />]))}
      {view === 'contracts' && table(t.contracts, [t.contract, t.providers, t.consumers], graph.contracts.map(row => [<code>{row.contract}</code>, <List values={row.providers} />, <List values={row.consumers} />]))}
      {view === 'links' && table(t.links, [t.contract, t.consumer, t.provider, t.level], graph.links.map(row => [<code>{row.contract}</code>, <code>{row.consumer}</code>, <code>{row.provider}</code>, row.level]))}
      {view === 'drift' && table(t.drift, [t.contract, t.consumer, t.provider, t.missing, t.unchecked], graph.drift.map(row => [
        <code>{row.contract}</code>, <code>{row.consumer}</code>, <code>{row.provider}</code>,
        <List values={Object.entries(row.missing).map(([path, level]) => `${path}: ${level}`)} />,
        <List values={Object.entries(row.unchecked).map(([path, open]) => `${path}: ${open}`)} />,
      ]))}
      {view === 'decisions' && <>
        {table(t.decisions, [t.contract, t.consumer, t.provider, t.decider, t.probability], graph.decisions.map(row => [
          <code>{row.contract}</code>, <code>{row.consumer}</code>, <code>{row.provider}</code>, row.by ?? t.notRecorded, row.p ?? t.notRecorded,
        ]))}
        {table(t.stale, [t.call, t.target, t.cause], graph.stale.map(row => [<code>{row.call}</code>, <code>{row.target}</code>, row.cause]))}
      </>}
    </>}
  </>;
}
