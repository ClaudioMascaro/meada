import { ApiError } from './api';
import { ruleProblem } from './account';
import type { AccountsCopy } from './accounts-i18n';

export type AccountRole = 'operator' | 'viewer';
export interface ManagedAccount {
  username: string; role: AccountRole; state: 'active' | 'suspended' | 'locked'; locked_until: string | null;
  change_required: boolean; password_changed: string; created: string; created_by: string | null; groups: string[];
  github_login?: string | null;
}

const PASSWORD_MIN = 12;
const PASSWORD_MAX = 128;
const USERNAME = /^[a-z0-9._-]{1,64}$/;

export function createCheck(username: string, password: string, t: AccountsCopy): string | undefined {
  return USERNAME.test(username) ? passwordCheck(password, t) : t.usernameInvalid;
}

export function passwordCheck(password: string, t: AccountsCopy): string | undefined {
  const length = [...password].length;
  return length < PASSWORD_MIN || length > PASSWORD_MAX ? t.passwordLength : undefined;
}

export function parseGroups(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).filter(Boolean))];
}

export function stateText(account: ManagedAccount, t: AccountsCopy): string {
  if (account.state === 'suspended') return t.suspended;
  if (account.state === 'locked') return `${t.lockedUntil} ${account.locked_until ?? ''}`.trim();
  return t.active;
}

export function accountsModel(accounts: ManagedAccount[], t: AccountsCopy) {
  return accounts.map(account => ({
    username: account.username,
    role: t[account.role],
    groups: account.groups.join(', ') || t.none,
    state: stateText(account, t),
    change: account.change_required ? t.yes : t.no,
    changed: account.password_changed,
    login: account.github_login ?? t.none,
  }));
}

export type Change =
  | { action: 'role'; role: AccountRole }
  | { action: 'groups'; groups: string[] }
  | { action: 'suspend' } | { action: 'reactivate' }
  | { action: 'reset_password'; password: string; change_required: boolean }
  | { action: 'remove_github' };

export function changeRoute(username: string): string { return `accounts/${encodeURIComponent(username)}`; }

export function refusalText(problem: unknown, t: AccountsCopy): string {
  if (problem instanceof ApiError) return ruleProblem(problem, t) ?? problem.message;
  return t.failed;
}
