import { startTestBackend } from '@backstage/backend-test-utils';

export async function run() {
  await startTestBackend({ features: [import('@scope/a'), import('@scope/b')] });
}

function other(o: object) {}

export function no() {
  other({ features: [import('@scope/a')] });
}
