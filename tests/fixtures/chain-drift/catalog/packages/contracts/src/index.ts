import { z } from "zod";

// The package's entry re-exports its files: by name, all of one, and the types of another.
export { Quote } from "./quote";
export * from "./shelf";
export { Receipt, Status, Tagged } from "./receipt";

// A ticket, the default export: there is no `row`.
const Ticket = z.object({ id: z.string(), seat: z.string() });
export default Ticket;
