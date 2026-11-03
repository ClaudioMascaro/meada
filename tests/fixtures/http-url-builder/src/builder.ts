const BASE = "https://api.example.com";

// A function that only builds the URL: its body is one returned string.
export const url = (p: string) => `${BASE}/${p}`;

// The same, with a lone `return`.
export function ownersUrl(p: string) {
  return `${BASE}/${p}`;
}
