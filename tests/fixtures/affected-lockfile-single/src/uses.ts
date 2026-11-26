import { map } from 'lodash';

export function double(xs: number[]) {
  return map(xs, (x) => x * 2);
}
