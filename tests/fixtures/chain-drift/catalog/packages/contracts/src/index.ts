import { z } from "zod";

// A quote of a product: there is no `discount`.
export const Quote = z.object({ sku: z.string(), price: z.number() });

// The fields every shelf answer starts from: there is no `aisle`.
export const SHELF = { warehouse: "w1", reserved: 0 };

// A constant Meada cannot read: a shelf listing spreading it is open.
export const LIMITS = Object.freeze({ max: 10 });

// A union lists no fields: a reader typed with it adds none.
export type Status = { paid: string } | { refunded: string };

// A receipt as its readers see it.
export interface Receipt {
  id: string;
  total: number;
  paidAt: string;
}
