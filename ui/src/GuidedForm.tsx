import { useEffect, useState } from 'react';
import type { FormEvent } from 'react';
import { PREFIX_MAX } from './catalog';
import { lines, removeAt, replaceAt, ROLES } from './guided';
import type { Entry, PeerDecl, ResourceDecl, ServeDecl } from './guided';
import type { GuidedCopy } from './guided-i18n';
import type { CatalogView } from './useCatalog';

/** What the page says of one entry, already in words: the form shows it and decides nothing. */
export interface EntryStatus { eligibility: string; ingestion: string; source?: string; diagnostics: string[] }

/** A textarea of one value per line. It keeps the raw text while the person types, so a line break is not eaten. */
function LinesField({ label, value, disabled, onChange }: { label: string; value: string[]; disabled: boolean; onChange: (lines: string[]) => void }) {
  const [raw, setRaw] = useState(value.join('\n'));
  useEffect(() => { if (lines(raw).join('\n') !== value.join('\n')) setRaw(value.join('\n')); }, [value, raw]);
  return <label>{label}<textarea rows={2} spellCheck={false} disabled={disabled} value={raw} onChange={event => { setRaw(event.target.value); onChange(lines(event.target.value)); }} /></label>;
}

function ServeFields({ serve, disabled, t, onChange }: { serve: ServeDecl | undefined; disabled: boolean; t: GuidedCopy; onChange: (serve: ServeDecl | undefined) => void }) {
  return <fieldset disabled={disabled}>
    <legend>{t.serve}</legend>
    <label><input type="checkbox" checked={serve !== undefined} onChange={event => onChange(event.target.checked ? {} : undefined)} />{t.declareServe}</label>
    {serve && <>
      <LinesField label={t.hosts} value={serve.hosts ?? []} disabled={disabled} onChange={hosts => onChange({ ...serve, hosts })} />
      <label>{t.prefix}<input value={serve.prefix ?? ''} onChange={event => onChange({ ...serve, prefix: event.target.value })} /></label>
      <label>{t.strip}<input value={serve.strip ?? ''} onChange={event => onChange({ ...serve, strip: event.target.value })} /></label>
    </>}
  </fieldset>;
}

function ResourceFields({ resources, disabled, t, onChange }: { resources: ResourceDecl[]; disabled: boolean; t: GuidedCopy; onChange: (resources: ResourceDecl[]) => void }) {
  return <fieldset disabled={disabled}>
    <legend>{t.resources}</legend>
    {resources.map((resource, at) => <fieldset key={at}>
      <legend>{t.resource(at + 1)}</legend>
      <label>{t.kind}<input value={resource.kind} onChange={event => onChange(replaceAt(resources, at, { ...resource, kind: event.target.value }))} /></label>
      <label>{t.id}<input value={resource.id} onChange={event => onChange(replaceAt(resources, at, { ...resource, id: event.target.value }))} /></label>
      <label>{t.role}<select value={resource.role} onChange={event => onChange(replaceAt(resources, at, { ...resource, role: event.target.value as ResourceDecl['role'] }))}>
        {ROLES.map(role => <option key={role} value={role}>{t[role]}</option>)}
      </select></label>
      <LinesField label={t.via} value={resource.via ?? []} disabled={disabled} onChange={via => onChange(replaceAt(resources, at, { ...resource, via }))} />
      <button type="button" className="secondary" aria-label={`${t.remove} ${t.resource(at + 1)}`} onClick={() => onChange(removeAt(resources, at))}>{t.remove}</button>
    </fieldset>)}
    <button type="button" className="secondary" onClick={() => onChange([...resources, { kind: '', id: '', role: 'read' }])}>{t.addResource}</button>
  </fieldset>;
}

function PeerFields({ peers, targets, disabled, t, onChange }: { peers: PeerDecl[]; targets: string[]; disabled: boolean; t: GuidedCopy; onChange: (peers: PeerDecl[]) => void }) {
  return <fieldset disabled={disabled}>
    <legend>{t.peers}</legend>
    {peers.map((peer, at) => <fieldset key={at}>
      <legend>{t.peer(at + 1)}</legend>
      <label>{t.peerRepo}<select value={peer.repo} onChange={event => onChange(replaceAt(peers, at, { ...peer, repo: event.target.value }))}>
        {!targets.includes(peer.repo) && <option value={peer.repo}>{peer.repo ? `${peer.repo} (${t.peerOutside})` : ''}</option>}
        {targets.map(target => <option key={target} value={target}>{target}</option>)}
      </select></label>
      <LinesField label={t.envRefs} value={peer.base_url_env ?? []} disabled={disabled} onChange={base_url_env => onChange(replaceAt(peers, at, { ...peer, base_url_env }))} />
      <button type="button" className="secondary" aria-label={`${t.remove} ${t.peer(at + 1)}`} onClick={() => onChange(removeAt(peers, at))}>{t.remove}</button>
    </fieldset>)}
    {targets.length === 0 ? <p className="notice">{t.noPeerTarget}</p>
      : <button type="button" className="secondary" onClick={() => onChange([...peers, { repo: targets[0]! }])}>{t.addPeer}</button>}
  </fieldset>;
}

/**
 * The guided controls over a named manifest (#345): repositories, and for each its serve settings,
 * resources and peers with their environment-variable names. It edits the entries the core reads;
 * the page rewrites the canonical text from them and checks it with the core, so this form holds no
 * rule about what an entry may contain. `status` is the eligibility, ingestion and effective
 * configuration of each entry, in words.
 */
export function GuidedForm({ entries, disabled, t, status, catalog, onChange }: {
  entries: Entry[]; disabled: boolean; t: GuidedCopy; status: Record<string, EntryStatus>; catalog: CatalogView; onChange: (entries: Entry[]) => void;
}) {
  const [typed, setTyped] = useState('');
  const [pick, setPick] = useState('');
  const present = new Set(entries.map(entry => entry.repo));
  const addable = catalog.listed.filter(identity => !present.has(identity));
  const pending = catalog.answer?.state === 'pending';
  function search(event: FormEvent) { event.preventDefault(); if (typed.length <= PREFIX_MAX) catalog.search(typed); }
  function add() {
    const identity = pick || addable[0];
    if (identity) { onChange([...entries, { repo: identity }]); setPick(''); }
  }
  return <section aria-label={t.guided}>
    <h4>{t.repositories}</h4>
    {entries.length === 0 && <p className="notice">{t.formEmpty}</p>}
    {entries.map((entry, at) => {
      const s = status[entry.repo];
      const targets = entries.map(other => other.repo).filter(repo => repo !== entry.repo);
      const set = (next: Entry) => onChange(replaceAt(entries, at, next));
      return <fieldset key={`${at}:${entry.repo}`} disabled={disabled}>
        <legend>{entry.repo}</legend>
        {s && <ul aria-label={`${entry.repo}: ${t.eligibility}`}>
          <li>{t.eligibility}: {s.eligibility}</li>
          <li>{t.ingestion}: {s.ingestion}</li>
          {s.source && <li>{t.source}: {s.source}</li>}
          {s.diagnostics.map(diagnostic => <li key={diagnostic}>{diagnostic}</li>)}
        </ul>}
        <ServeFields serve={entry.serve} disabled={disabled} t={t} onChange={serve => set({ ...entry, serve })} />
        <ResourceFields resources={entry.resource ?? []} disabled={disabled} t={t} onChange={resource => set({ ...entry, resource })} />
        <PeerFields peers={entry.peer ?? []} targets={targets} disabled={disabled} t={t} onChange={peer => set({ ...entry, peer })} />
        <button type="button" className="secondary" aria-label={`${t.removeRepository} ${entry.repo}`} onClick={() => onChange(removeAt(entries, at))}>{t.remove}</button>
      </fieldset>;
    })}
    <fieldset disabled={disabled || pending}>
      <legend>{t.add}</legend>
      {pending ? <p className="notice" role="status">{t.catalogPending}</p> : <>
        <form onSubmit={search} className="catalog-search">
          <label>{t.search}<input value={typed} maxLength={PREFIX_MAX} onChange={event => setTyped(event.target.value)} /></label>
          <button type="submit" className="secondary">{t.searchButton}</button>
        </form>
        {addable.length === 0 ? <p className="notice">{t.noneToAdd}</p> : <>
          <label>{t.addSelect}<select value={pick || addable[0]} onChange={event => setPick(event.target.value)}>
            {addable.map(identity => <option key={identity} value={identity}>{identity}</option>)}
          </select></label>
          <button type="button" onClick={add}>{t.addButton}</button>
        </>}
        {catalog.answer?.next != null && <button type="button" className="secondary" onClick={catalog.more}>{t.moreCatalog}</button>}
      </>}
    </fieldset>
  </section>;
}
