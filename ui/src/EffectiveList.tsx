import { coverageText, diagnosticText, sourceText } from './named';
import type { EffectiveView } from './named';
import type { NamedCopy } from './named-i18n';

/** The effective configuration of each entry, in text: its source, its coverage and what is ignored or not yet in effect. */
export function EffectiveList({ view, t, title }: { view: EffectiveView; t: NamedCopy; title?: string }) {
  return <section aria-label={title ?? t.effective}>
    <h3>{title ?? t.effective}</h3>
    <p className="notice">{view.generation ? `${t.generation}: ${view.generation}` : t.noGeneration}</p>
    {view.recorded === false && <p className="notice" role="status">{t.notRecorded}</p>}
    <ul aria-label={title ?? t.effective}>
      {view.entries.map(entry => <li key={entry.repo}>
        <strong>{entry.repo}</strong>: {sourceText(entry.source, t)} · {coverageText(entry.coverage, t)}
        {entry.diagnostics.length > 0 && <ul aria-label={entry.repo}>
          {entry.diagnostics.map((diagnostic, at) => <li key={`${diagnostic.code}:${diagnostic.detail ?? ''}:${at}`}>{diagnosticText(diagnostic, t)}</li>)}
        </ul>}
      </li>)}
    </ul>
  </section>;
}
