import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
const chained = backend.with();
chained.add(import('@scope/plugin-a'));
