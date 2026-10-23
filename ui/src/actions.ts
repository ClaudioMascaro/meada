import type { Config, Session } from './api';
import type { Generation } from './operations';
import { shellQuote } from './commands';

export interface Operation {
  action: 'set' | 'retain' | 'drop' | 'recover';
  route: 'pointer' | 'fence/recover';
  body: { action: string; generation: string } | { at_least?: string };
  command: string;
}

export function canOperate(session: Session, config: Config): boolean {
  return session.role === 'operator' && config.operations;
}

export function pointerActions(row: Pick<Generation, 'id' | 'retained' | 'current' | 'kept' | 'malformed'>,
  session: Session, config: Config, file: string): Operation[] {
  if (!canOperate(session, config) || row.malformed) return [];
  const actions: ('set' | 'retain' | 'drop')[] = [];
  if (row.retained) actions.push('set');
  if (!row.kept) actions.push('retain');
  if (row.retained && !row.current) actions.push('drop');
  return actions.map(action => ({ action, route: 'pointer', body: { action, generation: row.id },
    command: `meada snapshot pointer ${action} ${shellQuote(row.id)} --workspace ${shellQuote(file)}` }));
}

export function recoverAction(floor: string, file: string): Operation {
  const value = floor.trim();
  if (value && (!/^[0-9]+$/.test(value) || BigInt(value) > 18446744073709551615n)) throw new RangeError('Invalid fence');
  const atLeast = value ? BigInt(value).toString() : undefined;
  return { action: 'recover', route: 'fence/recover', body: atLeast === undefined ? {} : { at_least: atLeast },
    command: `meada snapshot fence recover${atLeast === undefined ? '' : ` --at-least ${atLeast}`} --workspace ${shellQuote(file)}` };
}
