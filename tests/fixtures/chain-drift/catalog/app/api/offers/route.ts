import { NextResponse } from "next/server";
import * as C from "@chain/contracts";

declare function load(): Promise<unknown>;

// The schema through a namespace import of the package.
export async function GET() {
  return NextResponse.json(C.Quote.parse(await load()));
}
