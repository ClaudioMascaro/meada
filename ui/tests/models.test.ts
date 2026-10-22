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
