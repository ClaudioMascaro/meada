import { createBackend } from '@backstage/backend-defaults';
export function wire(backend) { backend.add(import('@scope/plugin-a')); }
wire(createBackend());
