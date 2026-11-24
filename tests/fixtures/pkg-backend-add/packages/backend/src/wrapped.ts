import { createBackend } from './local-backend';
const backend = createBackend();
backend.add(import('@scope/plugin-a'));
