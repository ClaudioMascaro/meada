import { ApiError } from './api';
import type { AccountCopy } from './account-i18n';

function unavailable(problem: ApiError, t: AccountCopy): string | undefined {
  if (problem.status !== 503) return undefined;
  if (problem.code === 'busy') return t.signInBusy;
  if (problem.code === 'audit_unavailable') return t.signInAudit;
  return undefined;
}

export function signInProblem(problem: unknown, t: AccountCopy): string {
  if (problem instanceof ApiError) {
    if (problem.status === 401 && problem.code === 'invalid_credentials') return t.signInInvalid;
    const shared = unavailable(problem, t);
    if (shared) return shared;
  }
  return t.signInFailed;
}

export function ruleProblem(problem: ApiError, t: Record<string, string>): string | undefined {
  if (problem.status !== 422 || problem.code !== 'password_policy') return undefined;
  const rule = problem.body.rule;
  const known = typeof rule === 'string' ? t[`rule_${rule}`] : undefined;
  return known ?? `${t['rule_unknown']} ${problem.message}`;
}

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

export function changeCheck(current: string, next: string, repeat: string, t: AccountCopy): string | undefined {
  if (!current || !next || !repeat) return t.required;
  if (next !== repeat) return t.mismatch;
  return undefined;
}

export function githubProblem(problem: unknown, t: AccountCopy): string {
  if (problem instanceof ApiError) {
    const known: Record<string, string | undefined> = {
      '422 github_refused': t.githubRefused, '422 invalid_token': t.githubInvalidToken, '422 invalid_login': t.githubInvalidLogin,
      '409 login_taken': t.githubTaken, '503 github_unavailable': t.githubUnavailable, '503 audit_unavailable': t.signInAudit,
      '403 forbidden': t.githubForbidden,
    };
    const found = known[`${problem.status} ${problem.code}`];
    if (found) return found;
  }
  return t.githubFailed;
}

export interface AgentToken { token: string; expires_at: string; surface: string }

export function tokenProblem(problem: unknown, t: AccountCopy): string {
  if (problem instanceof ApiError) {
    if (problem.status === 422 && problem.code === 'invalid_hours') return t.tokenHours;
    if (problem.status === 403 && problem.code === 'forbidden') return t.tokenRefused;
    if (problem.status === 503 && problem.code === 'audit_unavailable') return t.signInAudit;
  }
  return t.tokenFailed;
}
