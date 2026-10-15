// A dependency declared in `devDependencies` crosses too.
import { greet } from '@example/kit';

export function smoke(): string {
  return greet('e2e');
}
