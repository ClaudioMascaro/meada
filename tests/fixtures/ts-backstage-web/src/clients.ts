import { Config } from '@backstage/config';

// The base URL in a field only the constructor writes, once: what `config.getString` reads.
export class OrdersClient {
  private readonly baseUrl: string;

  constructor(config: Config) {
    this.baseUrl = config.getString('api.baseUrl');
  }

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

// The same field public and mutable: any code may write it.
export class PublicClient {
  baseUrl: string;

  constructor(config: Config) {
    this.baseUrl = config.getString('api.baseUrl');
  }

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

// Written twice: by its initializer and by the constructor.
export class TwiceClient {
  private readonly baseUrl = 'http://localhost:7007';

  constructor(config: Config) {
    this.baseUrl = config.getString('api.baseUrl');
  }

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

// Written under a condition: the field holds the read, or nothing.
export class ConditionalClient {
  readonly #baseUrl: string;

  constructor(config: Config, remote: boolean) {
    if (remote) {
      this.#baseUrl = config.getString('api.baseUrl');
    }
  }

  list() {
    return fetch(`${this.#baseUrl}/orders`);
  }
}

// Written outside the constructor.
export class LateClient {
  private baseUrl: string;

  init(config: Config) {
    this.baseUrl = config.getString('api.baseUrl');
  }

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

// A parameter property and a constructor's parameter: the callers of `new` give the value.
export class ParameterClient {
  constructor(private readonly baseUrl: string) {}

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

export class AssignedClient {
  private readonly baseUrl: string;

  constructor(baseUrl: string) {
    this.baseUrl = baseUrl;
  }

  list() {
    return fetch(`${this.baseUrl}/orders`);
  }
}

export function makeClients(config: Config) {
  const base = config.getString('api.baseUrl');
  return [new ParameterClient(base), new AssignedClient(base)];
}
