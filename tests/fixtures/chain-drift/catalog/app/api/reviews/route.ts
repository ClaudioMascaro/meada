// An empty error answer: `new Response` with a literal 404 is left out of the shape.
export async function GET(request: Request) {
  if (!request.headers.get("x-product")) {
    return new Response(null, { status: 404 });
  }
  return Response.json({ stars: 5, count: 2 });
}
