import { ApiError } from './api';
import type { AccountCopy } from './account-i18n';

/** The two refusals a sign-in and a password change share: the hash limit and the audit that cannot be written. */
function unavailable(problem: ApiError, t: AccountCopy): string | undefined {
  if (problem.status !== 503) return undefined;
  if (problem.code === 'busy') return t.signInBusy;
  if (problem.code === 'audit_unavailable') return t.signInAudit;
  return undefined;
}

/** The one message a refused sign-in shows: wrong username, wrong password, suspended and locked read alike (the server answers one 401), while `busy` and `audit_unavailable` are named. */
export function signInProblem(problem: unknown, t: AccountCopy): string {
  if (problem instanceof ApiError) {
    if (problem.status === 401 && problem.code === 'invalid_credentials') return t.signInInvalid;
    const shared = unavailable(problem, t);
    if (shared) return shared;
  }
  return t.signInFailed;
}

/** A 422 `password_policy`: the server's rule code as the page's own copy, the server text only for a rule this page does not know. */
export function ruleProblem(problem: ApiError, t: Record<string, string>): string | undefined {
  if (problem.status !== 422 || problem.code !== 'password_policy') return undefined;
  const rule = problem.body.rule;
  const known = typeof rule === 'string' ? t[`rule_${rule}`] : undefined;
  return known ?? `${t['rule_unknown']} ${problem.message}`;
}

/** The message of a refused password change: the server's rule (its code, in the page's language), never the password. */
export function changeProblem(problem: unknown, t: AccountCopy): string {
  if (problem instanceof ApiError) {
    const rule = ruleProblem(problem, t);
    if (rule) return rule;
    if (problem.status === 401 && problem.code === 'invalid_credentials') return t.wrongCurrent;
    const shared = unavailable(problem, t);
    if (shared) return shared;
  }
  return t.changeFailed;
}

/** What the form checks before asking the server: every field filled and the new password typed twice. The server's policy stays the server's. */
export function changeCheck(current: string, next: string, repeat: string, t: AccountCopy): string | undefined {
  if (!current || !next || !repeat) return t.required;
  if (next !== repeat) return t.mismatch;
  return undefined;
}
