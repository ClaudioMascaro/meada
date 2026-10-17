import { z } from "zod";

// A quote of a product: there is no `discount`.
export const Quote = z.object({ sku: z.string(), price: z.number() });
