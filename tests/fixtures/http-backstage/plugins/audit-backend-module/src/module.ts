import { coreServices, createBackendModule } from '@backstage/backend-plugin-api';
import Router from 'express-promise-router';

// Its `pluginId` is no literal: the router stays at the root.
export const auditModule = createBackendModule({
  pluginId: process.env.AUDIT_PLUGIN_ID ?? 'audit',
  moduleId: 'events',
  register(reg) {
    reg.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/audit', async (_req, res) => {
          res.json([]);
        });
        httpRouter.use(router);
      },
    });
  },
});
