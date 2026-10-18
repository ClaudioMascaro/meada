export function farewell(name: string): string {
  return `bye ${name}`;
}

export class Client {
  static create(): Client {
    return new Client();
  }
}

// Back to the entry: a cycle of `export *`.
export * from './index';
