class Client {
  listen() { return 'client'; }
}
class Peer {
  listen() { return 'peer'; }
  then(resolve: (client: Client) => void) { resolve(new Client()); }
}
function open(): Peer { return new Peer(); }
export async function fromPeer() {
  const value = await open();
  return value.listen();
}

async function openResponse(): Promise<Response> { return new Response(); }
export async function fromResponse() {
  const value = await openResponse();
  return value.json();
}
