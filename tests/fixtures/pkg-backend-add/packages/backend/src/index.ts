import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
backend.add(import('@scope/plugin-a'));
backend.add(
  import('@scope/plugin-b'),
);
backend.add(import('@scope/plugin-c'));
backend.add(import('@scope/plugin-d'));
backend.add(import('@scope/plugin-e'));
backend.add(import('@scope/nodefault'));
backend.add(import('./local'));
backend.add(import('@backstage/plugin-external'));
backend.start();
