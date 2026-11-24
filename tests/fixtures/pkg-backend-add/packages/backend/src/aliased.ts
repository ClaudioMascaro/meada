import { createBackend } from '@backstage/backend-defaults';
const make = createBackend;
make.add(import('@scope/plugin-a'));
