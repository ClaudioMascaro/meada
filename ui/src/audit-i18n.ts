import type { Language } from './i18n';

const en = {
  audit: 'Audit log',
  auditIntro: 'Every write to the fleet and the workspace, newest first, as Postgres recorded it.',
  refresh: 'Refresh', records: 'Audit records', empty: 'No records.', older: 'Older records', newest: 'Newest records',
  time: 'Time (UTC)', via: 'Via', subject: 'Subject', login: 'Login', role: 'Role', owners: 'Owners',
  action: 'Action', arguments: 'Arguments', outcome: 'Outcome', message: 'Message', none: 'None',
  console: 'Console', cli: 'CLI', viewer: 'Viewer', operator: 'Operator', team_operator: 'Team operator',
  pending: 'Pending', accepted: 'Accepted', refused: 'Refused', failed: 'Failed',
};
export type AuditCopy = { [K in keyof typeof en]: string };
export const auditCopy: Record<Language, AuditCopy> = {
  en,
  'pt-BR': {
    audit: 'Registro de auditoria',
    auditIntro: 'Cada escrita na frota e no workspace, da mais recente para a mais antiga, como o Postgres registrou.',
    refresh: 'Atualizar', records: 'Registros de auditoria', empty: 'Nenhum registro.', older: 'Registros anteriores', newest: 'Registros mais recentes',
    time: 'Horário (UTC)', via: 'Via', subject: 'Sujeito', login: 'Login', role: 'Papel', owners: 'Proprietários',
    action: 'Ação', arguments: 'Argumentos', outcome: 'Resultado', message: 'Mensagem', none: 'Nenhum',
    console: 'Console', cli: 'CLI', viewer: 'Leitor', operator: 'Operador', team_operator: 'Operador de time',
    pending: 'Pendente', accepted: 'Aceita', refused: 'Recusada', failed: 'Falhou',
  },
};
