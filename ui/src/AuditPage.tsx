import { useEffect, useState } from 'react';
import { ApiError } from './api';
import type { Api } from './api';
import { auditModel, auditRoute } from './audit';
import type { AuditPage as Page } from './audit';
import { auditCopy } from './audit-i18n';
import { copy } from './i18n';
import type { Language } from './i18n';
import './operations.css';

const columns = ['time', 'via', 'subject', 'login', 'role', 'owners', 'action', 'arguments', 'outcome', 'message'] as const;

/** The global operator's audit page: newest first, one page at a time through `next`. */
export function AuditPage({ api, locale }: { api: Api; locale: Language }) {
  const t = auditCopy[locale];
  const common = copy[locale];
  const [before, setBefore] = useState<number>();
  const [page, setPage] = useState<Page>();
  const [problem, setProblem] = useState<unknown>();
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const controller = new AbortController();
    setPage(undefined); setProblem(undefined);
    api.request<Page>(auditRoute(before), { signal: controller.signal })
      .then(value => { if (!controller.signal.aborted) setPage(value); })
      .catch(error => { if (!controller.signal.aborted) setProblem(error); });
    return () => controller.abort();
  }, [api, before, attempt]);
  const rows = page ? auditModel(page, t) : [];
  return <>
    <div className="operation-heading"><h1>{t.audit}</h1>
      <button type="button" className="secondary" onClick={() => setAttempt(value => value + 1)}>{problem ? common.retry : t.refresh}</button>
    </div>
    <p className="intro">{t.auditIntro}</p>
    {problem ? <p className="error" role="alert">{problem instanceof ApiError ? problem.message : common.loadError}</p>
      : !page ? <p role="status">{common.loading}</p>
      : <section>
        <h2>{t.records}</h2>
        {rows.length === 0 ? <p className="notice">{t.empty}</p> :
          <div className="table-scroll" role="region" aria-label={t.records} tabIndex={0}>
            <table aria-label={t.records}>
              <thead><tr>{columns.map(column => <th scope="col" key={column}>{t[column]}</th>)}</tr></thead>
              <tbody>{rows.map(row => <tr key={row.id}>{columns.map(column => <td key={column}>{row[column]}</td>)}</tr>)}</tbody>
            </table>
          </div>}
        <div className="audit-pages">
          {before !== undefined && <button type="button" className="secondary" onClick={() => setBefore(undefined)}>{t.newest}</button>}
          {page.next !== null && <button type="button" onClick={() => setBefore(page.next ?? undefined)}>{t.older}</button>}
        </div>
      </section>}
  </>;
}
