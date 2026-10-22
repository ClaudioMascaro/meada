export type Tool = 'search' | 'context' | 'trace';
export interface QueryAnswer { text: string; generation: string | null }

export function queryBody(repository: string, generation: string, tool: Tool, value: string,
  options: { kind: string; direction: string; budget: string }) {
  const arguments_: Record<string, string | number> = { [tool === 'search' ? 'query' : tool === 'context' ? 'symbol' : 'from']: value };
  if (tool === 'search' && options.kind) arguments_.kind = options.kind;
  if (tool === 'trace') arguments_.direction = options.direction;
  if (options.budget) arguments_.budget = Number(options.budget);
  return { repository, ...generation ? { generation } : {}, tool, arguments: arguments_ };
}
