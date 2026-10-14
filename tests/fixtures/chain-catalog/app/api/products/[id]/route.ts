import { load } from "../../../../lib/products";

export async function GET(_request: Request, { params }: { params: { id: string } }) {
  return Response.json(load(params.id, 2));
}
