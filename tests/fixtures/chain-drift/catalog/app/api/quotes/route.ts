import { NextResponse } from "next/server";
import { Quote } from "@chain/contracts";

declare function load(): Promise<unknown>;

// The schema comes from the shared package, in this repository.
export async function GET() {
  return NextResponse.json(Quote.parse(await load()));
}
