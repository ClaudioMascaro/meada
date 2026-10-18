export function seal(): string {
  return 'one';
}

// A name the enclosing package, `@example/shared`, exports too: its candidates never take it.
export function normalize(raw: string): string {
  return raw;
}
