import { createBackend as real } from '@backstage/backend-defaults';
export function createBackend() { return real(); }
