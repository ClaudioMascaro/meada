import { expect, test } from 'vitest';
import config from './fixtures/config.json';
import session from './fixtures/session.json';
import manifest from './fixtures/manifest.json';
import { configModel, manifestModel, sessionModel } from '../src/models';
import { copy } from '../src/i18n';
import type { Session } from '../src/api';

test('configuration and session display the running version, operations and subject', () => {
  expect(configModel(config, copy.en)).toContainEqual({ label: 'Meada version', value: '0.1.0' });
  expect(configModel(config, copy.en)).toContainEqual({ label: 'Operations', value: 'Disabled' });
  expect(sessionModel(session as Session, copy.en)).toEqual([
    { label: 'Subject', value: 'alice-7c1e' }, { label: 'Role', value: 'Viewer' },
  ]);
  expect(sessionModel({ subject: null, role: 'viewer' }, copy.en)[0]?.value).toBe('Admitted by the reverse proxy');
});

test('a session under the GitHub grant provider shows its login and whether the GitHub grant is current', () => {
  for (const locale of ['en', 'pt-BR'] as const) {
    const t = copy[locale];
    const github: Session = { subject: 'ana', role: 'viewer', grant: [], login: 'ana-souza', github_host: 'github.com', github_grant: 'expired' };
    expect(sessionModel(github, t).slice(2)).toEqual([
      { label: t.githubLogin, value: 'ana-souza' }, { label: t.githubHost, value: 'github.com' }, { label: t.githubGrant, value: t.github_expired },
    ]);
    const { login: _, ...loginless } = github;
    expect(sessionModel({ ...loginless, github_grant: 'unread' }, t).slice(2).map(field => field.value)).toEqual([t.noLogin, 'github.com', t.github_unread]);
  }
});

test('manifest keeps secret references by name, errors with position and key, and names disabled sections', () => {
  const model = manifestModel(manifest, copy.en);
  expect(model.loaded).toContain('MEADA_VALKEY_URL');
  expect(model.loaded).toContain('/run/secrets/github.pem');
  expect(model.errors).toEqual([{
    file: 'ops/meada.workspace.toml', line: '6', key: 'ui.colour',
    message: '`ui.colour` is not a meada.workspace.toml key',
  }]);
  expect(model.modules).toContainEqual({ section: 'snapshot', feature: 'snapshot', state: 'Section absent' });
  expect(model.modules).toContainEqual({ section: 'github_app', feature: 'github-app', state: 'Feature absent' });
  expect(model.modules).toContainEqual({ section: 'ui', feature: 'ui', state: 'Available' });
});
