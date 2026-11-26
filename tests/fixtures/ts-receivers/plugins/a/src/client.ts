export default class Client {
  send() {}
  listen() {}
}
export { Client, Client as Sender };
declare function get(): Client;

export function fresh() {
  const c = new Client();
  c.send();
}
export function annotated() {
  const c: Client = get();
  c.send();
}
export function param(c: Client) { c.send(); }
export function optional(c?: Client) { c?.send(); }
export async function awaited(c: Client) { (await c).send(); }
export function parens(c: Client) { (c).send(); }
