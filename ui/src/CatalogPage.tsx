import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { ApiError } from './api';
import type { Api } from './api';
import { CHECK_IDENTITIES, PREFIX_MAX, catalogRoute, checkBody, checkText, reasonText, setText } from './catalog';
import type { CatalogAnswer, CheckAnswer } from './catalog';
import { catalogCopy } from './catalog-i18n';
import { copy } from './i18n';
import type { Language } from './i18n';
import './operations.css';

/**
 * The repository picker (#341): the repositories the session may select before a manifest exists,
 * a page at a time, with the state the catalog is in. Nothing is stored: the selection lives on
 * the page and a verification asks the server again, which decides.
 */
export function CatalogPage({ api, locale }: { api: Api; locale: Language }) {
  const t = catalogCopy[locale];
  const common = copy[locale];
  const [prefix, setPrefix] = useState('');
  const [typed, setTyped] = useState('');
  const [answer, setAnswer] = useState<CatalogAnswer>();
  const [more, setMore] = useState<string[]>([]);
  const [after, setAfter] = useState<string>();
  const [problem, setProblem] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  const [chosen, setChosen] = useState<string[]>([]);
  const [checked, setChecked] = useState<CheckAnswer>();
  const [checking, setChecking] = useState(false);
  useEffect(() => {
    const controller = new AbortController();
    setProblem(undefined);
    api.request<CatalogAnswer>(catalogRoute(prefix, after), { signal: controller.signal })
      .then(value => {
        if (controller.signal.aborted) return;
        setAnswer(value);
        setMore(previous => after === undefined ? value.repositories.map(r => r.identity) : [...previous, ...value.repositories.map(r => r.identity)]);
      })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, prefix, after, attempt]);
  function reload() { setAfter(undefined); setAnswer(undefined); setMore([]); setChecked(undefined); setAttempt(value => value + 1); }
  function search(event: FormEvent) {
    event.preventDefault();
    if (typed.length > PREFIX_MAX) return;
    setAfter(undefined); setAnswer(undefined); setMore([]); setPrefix(typed);
  }
  function toggle(identity: string) {
    setChecked(undefined);
    setChosen(previous => previous.includes(identity) ? previous.filter(i => i !== identity) : [...previous, identity]);
  }
  async function verify() {
    setChecking(true); setProblem(undefined);
    try {
      setChecked(await api.request<CheckAnswer>('catalog/check', {
        method: 'POST', headers: { 'Content-Type': 'application/json' }, body: checkBody(chosen),
      }));
    } catch (error) { setProblem(error); } finally { setChecking(false); }
  }
  const pending = answer?.state === 'pending';
  return <>
    <div className="operation-heading"><h1>{t.catalog}</h1>
      <button type="button" className="secondary" onClick={reload}>{problem ? common.retry : t.refresh}</button>
    </div>
    <p className="intro">{t.intro}</p>
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : common.loadError}</p> : null}
    {!answer && !problem ? <p role="status">{common.loading}</p> : null}
    {answer && <section aria-label={t.catalog}>
      <p role="status"><strong>{pending ? t.pendingState : t.ready}</strong>{pending ? `: ${reasonText(answer.reason, t)}` : ''}</p>
      <p className="notice">{setText(answer.set, t)}</p>
      {!pending && <>
        <form onSubmit={search} className="catalog-search">
          <label>{t.search}<input value={typed} maxLength={PREFIX_MAX} onChange={event => setTyped(event.target.value)} /></label>
          <button type="submit" className="secondary">{t.searchButton}</button>
        </form>
        <h2>{t.repositories}</h2>
        {more.length === 0 ? <p className="notice">{t.empty}</p> :
          <ul className="catalog-list" aria-label={t.repositories}>
            {more.map(identity => <li key={identity}>
              <label><input type="checkbox" checked={chosen.includes(identity)} disabled={!chosen.includes(identity) && chosen.length >= CHECK_IDENTITIES}
                onChange={() => toggle(identity)} aria-label={`${t.select} ${identity}`} />{identity}</label>
            </li>)}
          </ul>}
        {answer.next !== null && <button type="button" onClick={() => setAfter(answer.next ?? undefined)}>{t.more}</button>}
      </>}
      <h2>{t.selected}</h2>
      {chosen.length === 0 ? <p className="notice">{t.noneSelected}</p> : <ul aria-label={t.selected}>{chosen.map(identity => <li key={identity}>{identity}</li>)}</ul>}
      {chosen.length >= CHECK_IDENTITIES && <p className="notice">{t.selectionFull}</p>}
      <button type="button" disabled={chosen.length === 0 || checking} onClick={() => { void verify(); }}>{checking ? t.verifying : t.verify}</button>
      {checked && <section aria-label={t.verified}>
        <h2>{t.verified}</h2>
        {checked.state === 'pending' && <p role="status">{reasonText(checked.reason, t)}</p>}
        <ul>{checked.results.map(result => <li key={result.identity}>{result.identity}: {checkText(result.state, t)}</li>)}</ul>
      </section>}
    </section>}
  </>;
}
