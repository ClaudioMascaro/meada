// The fields every shelf answer starts from: there is no `aisle`.
export const SHELF = { warehouse: "w1", reserved: 0 };

// A constant Meada cannot read: a shelf listing spreading it is open.
export const LIMITS = Object.freeze({ max: 10 });
