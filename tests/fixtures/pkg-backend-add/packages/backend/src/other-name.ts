import { makeBackend } from '@backstage/backend-defaults';
const backend = makeBackend();
backend.add(import('@scope/plugin-a'));
