import { createSpecializedBackend } from '@backstage/backend-app-api';
const backend = createSpecializedBackend({});
backend.add(import('@scope/plugin-a'));
