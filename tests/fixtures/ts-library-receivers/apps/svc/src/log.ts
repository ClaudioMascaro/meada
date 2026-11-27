import type { Logger } from 'some-lib';

export class Store {
  info() {}
  get(k: string) {}
  json() {}
  parse(s: string) {}
  max(n: number) {}
  keys(o: object) {}
}
export class Audit implements Logger { info() {} }
export const q = { info() {} };

export function outside(l: Logger) { l.info(); }

export class Service {
  static log = makeLog();
  constructor(private log: Logger) {}
  instanceLog() { this.log.info(); }
  static staticLog() { (this.log).info(); }
}
