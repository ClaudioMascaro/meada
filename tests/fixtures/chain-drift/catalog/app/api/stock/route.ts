import { NextResponse } from "next/server";
import { LIMIT, TAGS } from "../../../lib/limits";

const HISTORY = [1, 2];

// Literals through an imported constant; `history` maps rows, so it is present and open.
export async function GET() {
  return NextResponse.json({
    sku: "p1",
    name: "Widget",
    count: LIMIT,
    tags: TAGS,
    history: HISTORY.map((at) => ({ at })),
  });
}

// A spread of an unknown value opens the whole object.
export async function POST(request: Request) {
  const extra = await request.json();
  return NextResponse.json({ ...extra, ok: true });
}
