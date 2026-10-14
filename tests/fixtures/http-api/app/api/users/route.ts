import { query } from "../../../lib/db";

export async function GET() {
  return Response.json(query("users"));
}

export async function POST(request: Request) {
  return Response.json(await request.json(), { status: 201 });
}
