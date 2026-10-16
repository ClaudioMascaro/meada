import { NextResponse } from "next/server";
import { Comment, Comments } from "../../../lib/schemas";

declare function load(): Promise<unknown>;

// The response is parsed with an imported schema; the 500 body is left out of the shape.
export async function GET() {
  try {
    const comments = Comments.parse(await load());
    return NextResponse.json(comments);
  } catch {
    return NextResponse.json({ error: "unavailable" }, { status: 500 });
  }
}

// The `data` of a safe parse; the 400 body is left out too.
export async function POST(request: Request) {
  const parsed = Comment.safeParse(await request.json());
  if (!parsed.success) {
    return NextResponse.json({ error: parsed.error.message }, { status: 400 });
  }
  return NextResponse.json(parsed.data, { status: 201 });
}
