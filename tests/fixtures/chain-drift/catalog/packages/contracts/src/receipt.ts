// A receipt as its readers see it.
export interface Receipt {
  id: string;
  total: number;
  paidAt: string;
}

// A union lists no fields: a reader typed with it adds none.
export type Status = { paid: string } | { refunded: string };

// Any label may be there: a route typed with `Tagged` is open at `labels`.
interface Labels {
  [key: string]: string;
}

export interface Tagged {
  name: string;
  labels: Labels;
}
