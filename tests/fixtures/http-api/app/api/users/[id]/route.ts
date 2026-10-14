import { query } from "../../../../lib/db";

export const GET = async (_request: Request, { params }: { params: { id: string } }) => {
  return Response.json(query("users", params.id));
};

export function DELETE() {
  return new Response(null, { status: 204 });
}
