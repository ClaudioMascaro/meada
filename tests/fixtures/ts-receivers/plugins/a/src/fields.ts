import { Client } from './client';
import { Client as Peer } from '../../b/src/client';

declare function other(): Client;

class S {
  constructor(private c: Client) {}
  go() { this.c.send(); }
}
class T {
  f: Client;
  go() { this.f.send(); }
}
class T2 {
  f: Client;
  reset() { this.f = other(); }
  go() { this.f.send(); }
}
class V {
  f = new Client();
  go() { this.f.send(); }
}
class U {
  single;
  constructor() { this.single = new Client(); }
  go() { this.single.send(); }
}
class Separate {
  once;
  constructor() { this.once = new Client(); }
  go() { this.once.send(); }
}
class W {
  static once = 0;
  once = 1;
  reset() { this.once = 2; }
}
class Changes {
  changed;
  constructor() { this.changed = new Client(); }
  reset() { this.changed = other(); }
  go() { this.changed.send(); }
}
class Parent {
  inherited;
  constructor() { this.inherited = new Client(); }
  go() { this.inherited.send(); }
}
class Child extends Parent {
  reset() { this.inherited = other(); }
}
class Shared {
  shared;
  constructor() { this.shared = new Client(); }
  go() { this.shared.send(); }
}
function reset(x: any) { x.shared = other(); }

const a = new Peer();
class V2 {
  a: Client;
  b = a.send();
}

// A write through a typed parameter belongs to that class.
function changeW(w: W) { w.once = 3; W.once = 4; }

class PrivateStore {
  private held;
  #hidden;
  constructor() {
    this.held = new Client();
    this.#hidden = new Client();
  }
  first() { this.held.send(); }
  second() { this.#hidden.send(); }
}
class OtherStore {
  #hidden;
  reset(x: any) { this.#hidden = 1; x.held = 1; }
}
