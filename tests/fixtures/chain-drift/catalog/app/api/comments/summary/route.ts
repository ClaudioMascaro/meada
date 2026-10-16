import { NextResponse } from "next/server";
import { z } from "zod";
import { Summary } from "../../../../lib/schemas";

declare function summarize(): Promise<z.infer<typeof Summary>>;

// Typed by the schema through the return type.
export async function GET(): Promise<NextResponse<z.infer<typeof Summary>>> {
  return NextResponse.json(await summarize());
}
