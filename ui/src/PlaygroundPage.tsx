import { useState } from 'react';
import type { Api } from './api';
import type { Language } from './i18n';
import { graphCopy } from './graph-i18n';
import { queryBody } from './playground';
import type { QueryAnswer, Tool } from './playground';
import './graph.css';

const kinds = ['symbol', 'contract', 'route', 'resource', 'channel', 'document', 'package', 'drift', 'community', 'process', 'ambiguous', 'unresolved'];

export function PlaygroundPage({ api, locale }: { api: Api; locale: Language }) {
  const t = graphCopy[locale];
  const [repository, setRepository] = useState('');
  const [generation, setGeneration] = useState('');
  const [tool, setTool] = useState<Tool>('search');
  const [value, setValue] = useState('');
  const [kind, setKind] = useState('');
  const [direction, setDirection] = useState('down');
  const [budget, setBudget] = useState('');
  const [answer, setAnswer] = useState<QueryAnswer>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit() {
    setBusy(true); setError(''); setAnswer(undefined);
    try {
      setAnswer(await api.request<QueryAnswer>('query', {
        method: 'POST', headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(queryBody(repository, generation, tool, value, { kind, direction, budget })),
      }));
    } catch (problem) { setError(problem instanceof Error ? problem.message : t.failed); }
    finally { setBusy(false); }
  }

  return <>
    <h1>{t.playground}</h1><p className="intro">{t.playgroundIntro}</p>
    <form className="graph-form" aria-label={t.playground} onSubmit={event => { event.preventDefault(); void submit(); }}>
      <label>{t.repository}<input required value={repository} onChange={event => setRepository(event.target.value)} placeholder="example.com/team/repository" autoCapitalize="none" spellCheck={false} /></label>
      <label>{t.generation}<input value={generation} onChange={event => setGeneration(event.target.value)} placeholder={t.current} pattern="[0-9a-f]{16}" autoCapitalize="none" spellCheck={false} /></label>
      <label>{t.tool}<select value={tool} onChange={event => {
        const next = event.target.value;
        if (next === 'search' || next === 'context' || next === 'trace') { setTool(next); setValue(''); }
      }}><option>search</option><option>context</option><option>trace</option></select></label>
      <label>{tool === 'search' ? t.query : tool === 'context' ? t.symbol : t.start}<input required value={value} onChange={event => setValue(event.target.value)} spellCheck={false} /></label>
      {tool === 'search' && <label>{t.kind}<select value={kind} onChange={event => setKind(event.target.value)}>
        <option value="">{t.anyKind}</option>{kinds.map(kind => <option key={kind}>{kind}</option>)}
      </select></label>}
      {tool === 'trace' && <label>{t.direction}<select value={direction} onChange={event => setDirection(event.target.value)}>
        <option value="down">{t.down}</option><option value="up">{t.up}</option>
      </select></label>}
      <label>{t.budget}<input type="number" min="1" max="4294967295" step="1" value={budget} onChange={event => setBudget(event.target.value)} placeholder="2000" /></label>
      <div className="graph-form-actions"><button type="submit" disabled={busy}>{busy ? t.loading : t.run}</button></div>
    </form>
    {error && <p role="alert" className="error">{error}</p>}
    {answer && <section>
      <h2>{t.answer}</h2><p className="file-path">{t.answeredBy}: <code>{answer.generation ?? t.local}</code></p>
      <pre className="configuration" role="region" aria-label={t.answer} tabIndex={0}>{answer.text}</pre>
    </section>}
  </>;
}
