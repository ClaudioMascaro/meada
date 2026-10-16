import { NextResponse } from "next/server";

// A receipt, without the `paidAt` the shared interface declares.
export async function GET() {
  return NextResponse.json({ id: "r1", total: 3 });
}
