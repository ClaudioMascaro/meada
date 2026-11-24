import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
const suffix = '';
backend.add(import(`@scope/plugin-a${suffix}`));
