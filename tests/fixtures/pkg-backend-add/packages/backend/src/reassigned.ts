import { createBackend } from '@backstage/backend-defaults';
let b = createBackend();
b = makeThing();
b.add(import('@scope/plugin-a'));
