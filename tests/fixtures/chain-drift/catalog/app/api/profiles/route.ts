import { NextResponse } from "next/server";
import { z } from "zod";

declare function load(): Promise<unknown>;

// The contract: a page of profiles. There is no `nickname`, `address.zip`, `count` or `pages`.
const Profile = z.object({
  id: z.string(),
  name: z.string(),
  address: z.object({ city: z.string() }),
});

const Page = z.object({ items: z.array(Profile), total: z.number() });

export async function GET() {
  return NextResponse.json(Page.parse(await load()));
}
