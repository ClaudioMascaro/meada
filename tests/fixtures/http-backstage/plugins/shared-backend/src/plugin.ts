import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { Router } from 'express';
import { PLUGIN_ID } from './id';

// A `pluginId` imported from another file is not read: the router stays at the root.
export const sharedPlugin = createBackendPlugin({
  pluginId: PLUGIN_ID,
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/ping', (req, res) => {
          res.json({ ok: true });
        });
        httpRouter.use(router);
      },
    });
  },
});
