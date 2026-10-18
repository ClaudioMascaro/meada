// The gateway serves `GET /api/ping` too, so a call with no host is ambiguous between the two.
export async function GET() {
  return Response.json({ ok: true });
}
