import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { Router } from 'express';

const BASE = 'chained';
// A `const` bound to another `const`, not to a literal: the router stays at the root.
const PLUGIN_ID = BASE;

export const chainedPlugin = createBackendPlugin({
  pluginId: PLUGIN_ID,
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/chain', (req, res) => {
          res.json([]);
        });
        httpRouter.use(router);
      },
    });
  },
});
