import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { Router } from 'express';

// A `pluginId` held by a constant of the file is read like a literal.
const PLUGIN_ID = 'catalog';

export const catalogPlugin = createBackendPlugin({
  pluginId: PLUGIN_ID,
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/entities', (req, res) => {
          res.json([]);
        });
        httpRouter.use(router);
      },
    });
  },
});
