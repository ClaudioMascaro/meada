import { createBackend } from '@backstage/backend-app-api';
const backend = createBackend();
backend.add(import('@scope/plugin-a'));
