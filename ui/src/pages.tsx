import type { MouseEvent, ReactNode } from 'react';
import type { Config, Manifest, Session } from './api';
import type { Copy, Language } from './i18n';
import { configModel, manifestModel, sessionModel } from './models';
import type { Field } from './models';

export const routes = [
  { path: '/config', label: 'config' },
  { path: '/session', label: 'session' },
  { path: '/manifest', label: 'manifest' },
] as const;

export function Shell({ path, locale, onLanguage, session, t, children, onNavigate }: {
  path: string; locale: Language; onLanguage: (language: Language) => void;
  session?: Session; t: Copy; children: ReactNode; onNavigate?: (path: string) => void;
}) {
  function follow(event: MouseEvent<HTMLAnchorElement>, target: string) {
    if (!onNavigate || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    event.preventDefault(); onNavigate(target);
  }
  return <div className="console-shell">
    <a className="skip-link" href="#main">{t.skip}</a>
    <header className="topbar">
      <a className="brand" href="/config" onClick={event => follow(event, "/config")}>Meada<span>{t.console}</span></a>
      <label className="language-picker">{t.language}
        <select value={locale} onChange={event => onLanguage(event.target.value === 'pt-BR' ? 'pt-BR' : 'en')}>
          <option value="pt-BR" lang="pt-BR">Português</option>
          <option value="en" lang="en">English</option>
        </select>
      </label>
    </header>
    <aside className="sidebar">
      <nav aria-label={t.navigation}>
        {routes.map(route => <a key={route.path} href={route.path} onClick={event => follow(event, route.path)} aria-current={path === route.path ? 'page' : undefined}>
          {t[route.label]}
        </a>)}
      </nav>
      {session && <div className="session-summary">
        <span className="role">{t[session.role]}</span>
        <span className="subject">{session.subject ?? t.anonymous}</span>
      </div>}
    </aside>
    <main id="main" tabIndex={-1}>{children}</main>
  </div>;
}

function Fields({ fields }: { fields: Field[] }) {
  return <dl className="fields">{fields.map(field => <div key={field.label}>
    <dt>{field.label}</dt><dd>{field.value}</dd>
  </div>)}</dl>;
}

export function ConfigPage({ config, t }: { config: Config; t: Copy }) {
  return <>
    <h1>{t.config}</h1><p className="intro">{t.configIntro}</p>
    <Fields fields={configModel(config, t)} />
    {!config.operations && <p className="notice">{t.readOnly}</p>}
  </>;
}

export function SessionPage({ session, t, onSignOut }: { session: Session; t: Copy; onSignOut?: () => void }) {
  return <>
    <h1>{t.session}</h1><p className="intro">{t.sessionIntro}</p>
    <Fields fields={sessionModel(session, t)} />
    {onSignOut && <button type="button" className="secondary" onClick={onSignOut}>{t.signOut}</button>}
  </>;
}

export function ManifestPage({ manifest, t }: { manifest: Manifest; t: Copy }) {
  const model = manifestModel(manifest, t);
  return <>
    <h1>{t.manifest}</h1><p className="intro">{t.manifestIntro}</p>
    <p className="file-path"><span>{t.file}</span><code>{manifest.file}</code></p>
    <section aria-labelledby="validation-title">
      <h2 id="validation-title">{t.validation}</h2>
      {model.errors.length === 0 ? <p className="notice">{t.valid}</p> :
        <div className="table-scroll" role="region" aria-label={t.validation} tabIndex={0}>
          <table><thead><tr><th>{t.file}</th><th>{t.line}</th><th>{t.key}</th><th>{t.problem}</th></tr></thead>
            <tbody>{model.errors.map((error, i) => <tr key={i}>
              <td><code>{error.file}</code></td><td>{error.line}</td><td><code>{error.key}</code></td><td>{error.message}</td>
            </tr>)}</tbody>
          </table>
        </div>}
    </section>
    <section aria-labelledby="modules-title">
      <h2 id="modules-title">{t.modules}</h2>
      <div className="table-scroll" role="region" aria-label={t.modules} tabIndex={0}>
        <table><thead><tr><th>{t.section}</th><th>{t.feature}</th><th>{t.available}</th></tr></thead>
          <tbody>{model.modules.map(module => <tr key={module.section}>
            <td><code>[{module.section}]</code></td><td><code>{module.feature}</code></td><td>{module.state}</td>
          </tr>)}</tbody>
        </table>
      </div>
    </section>
    <section aria-labelledby="loaded-title">
      <h2 id="loaded-title">{t.loaded}</h2>
      <pre className="configuration" role="region" aria-label={t.loaded} tabIndex={0}>{model.loaded}</pre>
    </section>
  </>;
}
