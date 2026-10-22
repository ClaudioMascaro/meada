import type { Language } from './i18n';

const en = {
  playground: 'Query playground', playgroundIntro: 'Ask the same read-only tools that agents use. Answers retain the MCP surface’s English text.',
  graph: 'Code graph', graphIntro: 'Load one repository export and explore every view locally.',
  repository: 'Repository', generation: 'Generation', current: 'Current generation', local: 'Local indexes',
  tool: 'Tool', query: 'Query', symbol: 'Symbol', start: 'Start', kind: 'Kind', anyKind: 'All kinds',
  direction: 'Direction', down: 'Downstream', up: 'Upstream', budget: 'Token budget (optional)',
  run: 'Run query', load: 'Load export', loading: 'Loading…', answer: 'Answer', answeredBy: 'Answered by generation',
  failed: 'The request failed. Try again.', noResults: 'No entries in this view.',
  communities: 'Communities', contracts: 'Contracts', links: 'Cross-repository links', drift: 'Shape drift', decisions: 'Decisions',
  view: 'Graph view', community: 'Community', members: 'Members', contract: 'Contract',
  providers: 'Providers', consumers: 'Consumers', provider: 'Provider', consumer: 'Consumer',
  level: 'Level', missing: 'Missing paths and levels', unchecked: 'Unchecked paths and open parts',
  decider: 'Decided by', probability: 'Probability', stale: 'Stale decisions', call: 'Call site', target: 'Target', cause: 'Cause',
  notRecorded: 'Not recorded', exportLoaded: 'Loaded repository',
} as const;
export type GraphCopy = { [K in keyof typeof en]: string };
export const graphCopy: Record<Language, GraphCopy> = {
  en,
  'pt-BR': {
    playground: 'Playground de consultas', playgroundIntro: 'Consulte as mesmas ferramentas de leitura usadas por agentes. As respostas mantêm o texto em inglês da superfície MCP.',
    graph: 'Grafo de código', graphIntro: 'Carregue a exportação de um repositório e explore todas as visões localmente.',
    repository: 'Repositório', generation: 'Geração', current: 'Geração atual', local: 'Índices locais',
    tool: 'Ferramenta', query: 'Consulta', symbol: 'Símbolo', start: 'Início', kind: 'Tipo', anyKind: 'Todos os tipos',
    direction: 'Direção', down: 'Descendente', up: 'Ascendente', budget: 'Orçamento de tokens (opcional)',
    run: 'Executar consulta', load: 'Carregar exportação', loading: 'Carregando…', answer: 'Resposta', answeredBy: 'Geração que respondeu',
    failed: 'A solicitação falhou. Tente novamente.', noResults: 'Nenhuma entrada nesta visão.',
    communities: 'Comunidades', contracts: 'Contratos', links: 'Ligações entre repositórios', drift: 'Divergência de shape', decisions: 'Decisões',
    view: 'Visão do grafo', community: 'Comunidade', members: 'Membros', contract: 'Contrato',
    providers: 'Provedores', consumers: 'Consumidores', provider: 'Provedor', consumer: 'Consumidor',
    level: 'Nível', missing: 'Caminhos ausentes e níveis', unchecked: 'Caminhos não verificados e partes abertas',
    decider: 'Decidido por', probability: 'Probabilidade', stale: 'Decisões obsoletas', call: 'Call site', target: 'Destino', cause: 'Causa',
    notRecorded: 'Não registrado', exportLoaded: 'Repositório carregado',
  },
};
