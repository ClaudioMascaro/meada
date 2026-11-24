import a, { aPlugin, S } from '@scope/a';
import handler from '@scope/c';
export function useAll() { a(); aPlugin(); handler(); fetch(`${S.A}/x`); }
