import { createBackend } from '@backstage/backend-defaults';
const backend = createBackend();
try {
  run();
} catch (backend) {
  backend.add(import('@scope/plugin-a'));
}
export const wire = ({ backend }) => backend.add(import('@scope/plugin-a'));
