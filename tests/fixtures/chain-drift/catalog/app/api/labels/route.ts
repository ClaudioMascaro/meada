import { NextResponse } from "next/server";

// `labels` has an index signature: any key may be there, though the literal lists one.
interface Labels {
  [key: string]: string;
}

interface Reply {
  ok: boolean;
  labels: Labels;
}

export async function GET(): Promise<NextResponse<Reply>> {
  return NextResponse.json({ ok: true, labels: { title: "Title" } });
}
