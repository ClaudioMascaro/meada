import * as d from '@backstage/backend-defaults';
const backend = d.createBackend();
backend.add(import('@scope/plugin-a'));
