import { NextResponse } from "next/server";
import Ticket from "@chain/contracts";

declare function load(): Promise<unknown>;

// The package's default export.
export async function GET() {
  return NextResponse.json(Ticket.parse(await load()));
}
