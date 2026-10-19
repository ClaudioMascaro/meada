export async function GET(_request: Request, { params }: { params: { sku: string } }) {
  return Response.json({ sku: params.sku, price: 10 });
}
