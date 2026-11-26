export class Base {
  send() {}
  listen() {}
  own() { this.send(); }
  wrapped() { (this).send(); }
  cross() {}
}
export class Sub extends Base {
  send() { super.send(); }
  inherited() { this.listen(); }
}
export function onBase(b: Base) { b.send(); }
export function onSub(s: Sub) { s.send(); }
export function onlyBase(s: Sub) { s.listen(); }
export function onRemoteBase(b: Base) { b.cross(); }
