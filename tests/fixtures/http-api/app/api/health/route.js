import { withLogging } from "../../../lib/log";

// Not a function definition: the route file itself serves it.
export const GET = withLogging(() => new Response("ok"));
