function label(n: number): string {
  return `#${n}`.trim();
}

export function Badge({ n }: { n: number }) {
  return <span>{label(n)}</span>;
}
