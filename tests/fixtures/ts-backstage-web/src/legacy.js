// A JavaScript file: its private field is left unread, as before.
export class LegacyClient {
  #baseUrl;

  constructor(config) {
    this.#baseUrl = config.getString('api.baseUrl');
  }

  list() {
    return fetch(`${this.#baseUrl}/orders`);
  }
}
