export function removeUser(_req: unknown, res: { status(code: number): { end(): void } }) {
  res.status(204).end();
}
