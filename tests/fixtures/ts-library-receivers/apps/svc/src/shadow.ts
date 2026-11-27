export class Map { get(k: string) {} }
export function shadowMap(k: string) { const m = new Map(); m.get(k); }
export function shadowConsole() { const console = makeLog(); console.info(); }
