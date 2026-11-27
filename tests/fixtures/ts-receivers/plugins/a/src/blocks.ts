import { Client } from './client';
import { Client as Peer } from '../../b/src/client';

export function separate() {
  {
    const c = new Client();
    c.send();
  }
  {
    const c = new Peer();
    c.listen();
  }
}

const c = new Peer();
export function outsideBlock() {
  { const c = new Client(); }
  c.send();
}
export function insideTry() {
  try {
    const c = new Client();
    c.send();
  } catch {}
}

export function fallthrough() {
  switch (0) {
    case 0:
      const c = new Client();
    default:
      return c.send();
  }
}

export function bracedFallthrough() {
  switch (0) {
    case 0: {
      const c = new Client();
    }
    default:
      return c.send();
  }
}

export function beforeDeclaration() {
  c.send();
  const c = new Client();
}
