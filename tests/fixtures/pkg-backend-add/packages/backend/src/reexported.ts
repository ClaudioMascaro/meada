import { createBackend } from './reexport-backend';
const backend = createBackend();
backend.add(import('@scope/plugin-a'));
