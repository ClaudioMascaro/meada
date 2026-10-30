import type { Language } from './i18n';

const en = {
  catalog: 'Repository catalog',
  intro: 'Repositories you may select for a workspace: administrator-allowed, covered by the GitHub App, and where your GitHub login holds an accepted role. Discovery creates no workspace and indexes nothing.',
  ready: 'Ready', pendingState: 'Pending verification',
  no_set: 'The job has not written a permissions set yet, so nothing can be selected.',
  stale_set: 'The newest permissions set is older than allowed, so nothing can be selected until the job writes another.',
  missing_login: 'Your token carries no valid GitHub login, so nothing can be selected for you.',
  noSet: 'No permissions set yet.',
  setAge: (finished: string, age: number, max: number) => `Permissions set from ${finished}, ${age} s old (at most ${max} s).`,
  repositories: 'Repositories you may select', empty: 'No repository to select.',
  search: 'Identity prefix', searchButton: 'Search', more: 'More repositories', refresh: 'Refresh',
  select: 'Select', selected: 'Selected (kept only on this page)', noneSelected: 'None selected.',
  verify: 'Verify the selection', verifying: 'Verifying…', verified: 'Verification',
  selectable: 'Selectable', unavailable: 'Unavailable', pending: 'Pending',
  selectionFull: 'At most 50 repositories can be verified at once.',
};
export type CatalogCopy = { [K in keyof typeof en]: (typeof en)[K] extends string ? string : (typeof en)[K] };
export const catalogCopy: Record<Language, CatalogCopy> = {
  en,
  'pt-BR': {
    catalog: 'Catálogo de repositórios',
    intro: 'Repositórios que você pode selecionar para um workspace: permitidos pelo administrador, cobertos pelo GitHub App e nos quais o seu login do GitHub tem um papel aceito. A descoberta não cria workspace e não indexa nada.',
    ready: 'Pronto', pendingState: 'Verificação pendente',
    no_set: 'O job ainda não gravou um conjunto de permissões, então nada pode ser selecionado.',
    stale_set: 'O conjunto de permissões mais novo é mais antigo que o permitido, então nada pode ser selecionado até o job gravar outro.',
    missing_login: 'O seu token não traz um login válido do GitHub, então nada pode ser selecionado para você.',
    noSet: 'Ainda não há conjunto de permissões.',
    setAge: (finished: string, age: number, max: number) => `Conjunto de permissões de ${finished}, com ${age} s (no máximo ${max} s).`,
    repositories: 'Repositórios que você pode selecionar', empty: 'Nenhum repositório para selecionar.',
    search: 'Prefixo da identidade', searchButton: 'Buscar', more: 'Mais repositórios', refresh: 'Atualizar',
    select: 'Selecionar', selected: 'Selecionados (mantidos só nesta página)', noneSelected: 'Nenhum selecionado.',
    verify: 'Verificar a seleção', verifying: 'Verificando…', verified: 'Verificação',
    selectable: 'Selecionável', unavailable: 'Indisponível', pending: 'Pendente',
    selectionFull: 'No máximo 50 repositórios podem ser verificados de uma vez.',
  },
};
