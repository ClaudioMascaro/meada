import { ApiError } from './api';
import { ruleProblem } from './account';
import type { AccountsCopy } from './accounts-i18n';

export type AccountRole = 'operator' | 'viewer';
/** One row of `GET accounts` (never a hash). */
export interface ManagedAccount {
  username: string; role: AccountRole; state: 'active' | 'suspended' | 'locked'; locked_until: string | null;
  change_required: boolean; password_changed: string; created: string; created_by: string | null; groups: string[];
}

const PASSWORD_MIN = 12;
const PASSWORD_MAX = 128;
/** The server's username pattern (`username_ok`): 1 to 64 of lowercase letters, digits, `.`, `-` and `_`. */
const USERNAME = /^[a-z0-9._-]{1,64}$/;

/** What the create form checks before asking the server; the server's policy (common passwords, the username) stays the server's. */
export function createCheck(username: string, password: string, t: AccountsCopy): string | undefined {
  return USERNAME.test(username) ? passwordCheck(password, t) : t.usernameInvalid;
}

/** The password's length in Unicode scalar values, as the server counts it. */
export function passwordCheck(password: string, t: AccountsCopy): string | undefined {
  const length = [...password].length;
  return length < PASSWORD_MIN || length > PASSWORD_MAX ? t.passwordLength : undefined;
}

/** A comma or whitespace separated list as the groups an account holds, without repeats. */
export function parseGroups(text: string): string[] {
  return [...new Set(text.split(/[\s,]+/).filter(Boolean))];
}

/** The state as text: suspended and locked accounts are marked, a locked one with the time its lock ends. */
export function stateText(account: ManagedAccount, t: AccountsCopy): string {
  if (account.state === 'suspended') return t.suspended;
  if (account.state === 'locked') return `${t.lockedUntil} ${account.locked_until ?? ''}`.trim();
  return t.active;
}

/** Each account as the list shows it, every field as text. */
export function accountsModel(accounts: ManagedAccount[], t: AccountsCopy) {
  return accounts.map(account => ({
    username: account.username,
    role: t[account.role],
    groups: account.groups.join(', ') || t.none,
    state: stateText(account, t),
    change: account.change_required ? t.yes : t.no,
    changed: account.password_changed,
  }));
}

export type Change =
  | { action: 'role'; role: AccountRole }
  | { action: 'groups'; groups: string[] }
  | { action: 'suspend' } | { action: 'reactivate' }
  | { action: 'reset_password'; password: string; change_required: boolean };

/** The route of one account's change: the username is the validated pattern, but is encoded all the same. */
export function changeRoute(username: string): string { return `accounts/${encodeURIComponent(username)}`; }

/** A refusal as the page shows it: the password rule in the page's language, any other refusal exactly as the server worded it. */
export function refusalText(problem: unknown, t: AccountsCopy): string {
  if (problem instanceof ApiError) return ruleProblem(problem, t) ?? problem.message;
  return t.failed;
}
