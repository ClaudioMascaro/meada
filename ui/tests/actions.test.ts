import { expect, test } from 'vitest';
import { pointerActions, recoverAction } from '../src/actions';
import config from './fixtures/config.json';

test('viewers and operators with operations off have no generation actions', () => {
  const row = { id: 'aaaaaaaaaaaaaaaa', retained: true, current: false, kept: true };
  expect(pointerActions(row, { role: 'viewer', subject: 'viewer' }, { ...config, operations: true }, 'ops.toml')).toEqual([]);
  expect(pointerActions(row, { role: 'operator', subject: 'operator' }, { ...config, operations: false }, 'ops.toml')).toEqual([]);
});

test('fence recovery preserves every u64 digit and validates the optional floor before confirmation', () => {
  expect(recoverAction('', 'ops.toml')).toEqual({ action: 'recover', route: 'fence/recover', body: {},
    command: 'meada snapshot fence recover --workspace ops.toml' });
  expect(recoverAction('18446744073709551615', 'ops.toml')).toEqual({
    action: 'recover', route: 'fence/recover', body: { at_least: '18446744073709551615' },
    command: 'meada snapshot fence recover --at-least 18446744073709551615 --workspace ops.toml',
  });
  expect(recoverAction('0009007199254740993', 'ops.toml').body).toEqual({ at_least: '9007199254740993' });
  for (const floor of ['18446744073709551616', '-1', '1.1', '1e3', 'token.jwt.secret']) {
    expect(() => recoverAction(floor, 'ops.toml')).toThrow(RangeError);
  }
});

test('an operator can set retained generations, retain unkept documents and drop only a noncurrent retention', () => {
  const session = { role: 'operator', subject: 'operator' } as const;
  const enabled = { ...config, operations: true };
  const actions = (state: { retained: boolean; current: boolean; kept: boolean }) =>
    pointerActions({ id: 'aaaaaaaaaaaaaaaa', ...state }, session, enabled, "ops/team's console.toml");
  expect(actions({ retained: true, current: true, kept: false }).map(action => action.action)).toEqual(['set', 'retain']);
  expect(actions({ retained: true, current: false, kept: true }).map(action => action.action)).toEqual(['set', 'drop']);
  expect(actions({ retained: false, current: false, kept: false }).map(action => action.action)).toEqual(['retain']);
  expect(actions({ retained: false, current: false, kept: false })[0]).toEqual({
    action: 'retain', route: 'pointer', body: { action: 'retain', generation: 'aaaaaaaaaaaaaaaa' },
    command: `meada snapshot pointer retain aaaaaaaaaaaaaaaa --workspace 'ops/team'"'"'s console.toml'`,
  });
});
