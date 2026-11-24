import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
const name = '@scope/plugin-a';
backend.add(plugin);
backend.add(makeFeature('@scope/plugin-a'));
backend.add(import(name));
backend.add(import('@scope/plugin-a'), import('@scope/plugin-b'));
backend.add();
backend.add(import(pick()));
