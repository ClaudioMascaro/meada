import fp from 'lodash/fp';

export function first(xs: number[]) {
  return fp.head(xs);
}
