import { NextResponse } from "next/server";

function ok(body: object) {
  return NextResponse.json(body);
}

// The second response goes through a helper Meada does not read: the shape is open at the root.
export async function GET(request: Request) {
  if (new URL(request.url).searchParams.has("hit")) {
    return NextResponse.json({ cached: true });
  }
  return ok({ user: "u1" });
}
