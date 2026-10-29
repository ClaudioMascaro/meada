import type { AuditCopy } from './audit-i18n';

export interface AuditRecord {
  id: number; time: string; via: 'console' | 'cli'; subject: string | null; login: string | null;
  role: 'viewer' | 'operator' | 'team_operator'; owners: string[] | null; action: string;
  arguments: Record<string, string>; outcome: 'pending' | 'accepted' | 'refused' | 'failed'; message: string | null;
}
export interface AuditPage { records: AuditRecord[]; next: number | null }

/** The route of the page after `before` (the newest without it). */
export function auditRoute(before?: number): string {
  return before === undefined ? 'audit' : `audit?${new URLSearchParams({ before: String(before) })}`;
}

/** Each record as the page shows it, every field as text. */
export function auditModel(page: AuditPage, t: AuditCopy) {
  return page.records.map(record => ({
    id: record.id,
    time: record.time,
    via: t[record.via],
    subject: record.subject ?? t.none,
    login: record.login ?? t.none,
    role: t[record.role],
    owners: record.owners?.join(', ') || t.none,
    action: record.action,
    arguments: Object.entries(record.arguments).map(([name, value]) => `${name}: ${value}`).join(', ') || t.none,
    outcome: t[record.outcome],
    message: record.message ?? t.none,
  }));
}
