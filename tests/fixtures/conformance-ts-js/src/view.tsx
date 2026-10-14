import { format } from '@/util';

export function render(n: number) {
  return <b>{format(n)}</b>;
}
