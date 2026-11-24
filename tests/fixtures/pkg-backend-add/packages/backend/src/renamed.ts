import { createBackend as make } from '@backstage/backend-defaults';
const backend = make();
backend.add(import('@scope/plugin-a'));
