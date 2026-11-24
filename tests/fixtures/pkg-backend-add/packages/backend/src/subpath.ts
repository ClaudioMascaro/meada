import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
backend.add(import('@scope/plugin-f/alpha'));
