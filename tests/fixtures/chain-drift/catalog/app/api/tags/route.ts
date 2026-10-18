import { NextResponse } from "next/server";
import type { Tagged } from "@chain/contracts";

// The return type comes from the package: its index signature opens `labels`.
export async function GET(): Promise<NextResponse<Tagged>> {
  return NextResponse.json({ name: "n", labels: { color: "red" } });
}
