import type { Config, Manifest, Session } from './api';
import type { Copy } from './i18n';

export interface Field { label: string; value: string }

export function configModel(config: Config, t: Copy): Field[] {
  return [
    { label: t.version, value: config.version },
    { label: t.operations, value: config.operations ? t.enabled : t.disabled },
    { label: t.admission, value: config.issuer ? t.authenticated : t.proxy },
    ...config.issuer ? [{ label: t.issuer, value: config.issuer }] : [],
  ];
}

export function sessionModel(session: Session, t: Copy): Field[] {
  return [
    { label: t.subject, value: session.subject ?? t.anonymous },
    { label: t.role, value: t[session.role] },
    ...session.github_grant ? [
      { label: t.githubLogin, value: session.login ?? t.noLogin },
      { label: t.githubHost, value: session.github_host ?? '' },
      { label: t.githubGrant, value: t[`github_${session.github_grant}`] },
    ] : [],
  ];
}

export function manifestModel(manifest: Manifest, t: Copy) {
  return {
    loaded: JSON.stringify(manifest.manifest, null, 2),
    modules: manifest.modules.map(module => ({
      section: module.section, feature: module.feature,
      state: !module.built ? t.missingFeature : Object.hasOwn(manifest.manifest, module.section) ? t.available : t.missingSection,
    })),
    errors: manifest.errors.map(error => {
      const match = /^(.*):(\d+)(?::\s*(.*))?$/.exec(error);
      const message = match?.[3] ?? error;
      return {
        file: match?.[1] ?? manifest.file, line: match?.[2] ?? '',
        key: /`([^`]+)`/.exec(message)?.[1] ?? '', message,
      };
    }),
  };
}
