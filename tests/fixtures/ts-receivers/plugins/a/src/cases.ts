class Implemented { notify() {} }
class Impl implements Implemented { notify() {} }
export function structuralClass(c: Implemented) { c.notify(); }

class Grand { relay() {} }
class Middle extends Grand {}
class Leaf extends Middle { relay() {} }
export function grandchild(c: Grand) { c.relay(); }

class Root { hit() {} }
class Inherits extends Root {
  run() { this.hit(); }
}
class Overrides extends Inherits { hit() {} }

class BaseField { ping() {} }
class Sub2 extends BaseField {
  ping = () => {};
  run() { this.ping(); }
}
export function shadow(c: Sub2) { c.ping(); }
