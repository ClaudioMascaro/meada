import { expect, test } from 'vitest';
import { changeCheck, changeProblem, signInProblem } from '../src/account';
import { accountCopy } from '../src/account-i18n';
import { ApiError } from '../src/api';

const refuse = (status: number, code: string, body: Record<string, unknown> = {}) => new ApiError(status, code, `server ${code}`, body);

test('both languages provide every account copy key', () => {
  expect(Object.keys(accountCopy['pt-BR']).sort()).toEqual(Object.keys(accountCopy.en).sort());
  for (const language of Object.values(accountCopy)) expect(Object.values(language).every(Boolean)).toBe(true);
});

test.each(['en', 'pt-BR'] as const)('a refused sign-in is one generic message, busy and audit_unavailable are distinct in %s', locale => {
  const t = accountCopy[locale];
  const invalid = signInProblem(refuse(401, 'invalid_credentials'), t);
  expect(invalid).toBe(t.signInInvalid);
  const busy = signInProblem(refuse(503, 'busy'), t);
  const audit = signInProblem(refuse(503, 'audit_unavailable'), t);
  expect(new Set([invalid, busy, audit, signInProblem(new Error('network'), t)]).size).toBe(4);
  expect(busy).toBe(t.signInBusy);
  expect(audit).toBe(t.signInAudit);
  // The server's own text is never shown for a sign-in.
  expect(`${invalid}${busy}${audit}`).not.toContain('server');
});

test.each(['en', 'pt-BR'] as const)('a refused password change names the rule in %s, with the server text only for an unknown rule', locale => {
  const t = accountCopy[locale];
  for (const rule of ['too_short', 'too_long', 'username', 'current_password'] as const) {
    expect(changeProblem(refuse(422, 'password_policy', { rule }), t)).toBe(t[`rule_${rule}`]);
  }
  expect(changeProblem(refuse(422, 'password_policy', { rule: 'novel' }), t)).toBe(`${t.rule_unknown} server password_policy`);
  expect(changeProblem(refuse(401, 'invalid_credentials'), t)).toBe(t.wrongCurrent);
  expect(changeProblem(refuse(503, 'busy'), t)).toBe(t.signInBusy);
  expect(changeProblem(refuse(503, 'audit_unavailable'), t)).toBe(t.signInAudit);
  expect(changeProblem(refuse(500, 'http'), t)).toBe(t.changeFailed);
});

test('the form asks for every field and the new password twice before the server', () => {
  const t = accountCopy.en;
  expect(changeCheck('', 'a', 'a', t)).toBe(t.required);
  expect(changeCheck('c', 'a', '', t)).toBe(t.required);
  expect(changeCheck('c', 'one', 'two', t)).toBe(t.mismatch);
  expect(changeCheck('c', 'same', 'same', t)).toBeUndefined();
});
