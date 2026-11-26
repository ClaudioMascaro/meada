import { map } from 'lodash';

export function f(xs: number[]) {
  return map(xs, (x) => x + 1);
}
