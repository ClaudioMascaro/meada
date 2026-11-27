import type { Outside } from 'some-lib';

export const v: Outside = { check() {} };
export function literal() { v.check(); }

class Store { check() {} }
export function nested() {
  const value: Outside = { check() {} };
  value.check();
}
export class Service {
  value: Outside = { check() {} };
  checkValue() { this.value.check(); }
}
