import { coreServices } from '@backstage/backend-plugin-api';
import Router from 'express-promise-router';
import { createBackendModule } from './framework';

// Not imported from a `@backstage/` package: it mounts nothing, and its router stays at the root.
export const localModule = createBackendModule({
  pluginId: 'local',
  moduleId: 'items',
  register(reg) {
    reg.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/local', async (_req, res) => {
          res.json([]);
        });
        httpRouter.use(router);
      },
    });
  },
});
