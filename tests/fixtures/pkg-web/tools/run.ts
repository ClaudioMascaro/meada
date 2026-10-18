// The nearest manifest does not declare `@example/kit` (only the root one does): no hop.
import { greet } from '@example/kit';

export function run(): string {
  return greet('tools');
}
