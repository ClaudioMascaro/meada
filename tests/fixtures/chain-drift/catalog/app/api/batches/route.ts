import { NextResponse } from "next/server";

// Rows with a nested object, and a page with one field: the consumers destructure and iterate.
export async function GET() {
  return NextResponse.json({
    items: [{ id: "b1", qty: 2, owner: { name: "ana" } }],
    page: { number: 1 },
  });
}
