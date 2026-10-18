const SKUS = ["p1"];

export async function GET(_request: Request, { params }: { params: { sku: string } }) {
  if (!SKUS.includes(params.sku)) {
    // An error body: left out of the shape.
    return Response.json({ error: "unknown sku" }, { status: 404 });
  }
  return Response.json({ sku: params.sku, count: 1 });
}
