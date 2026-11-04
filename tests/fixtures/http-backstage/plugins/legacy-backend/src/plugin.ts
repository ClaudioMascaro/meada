import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { Router } from 'express';

const id = process.env.LEGACY_PLUGIN_ID ?? 'legacy';

// Its `pluginId` is no literal: the router stays at the root.
export const legacyPlugin = createBackendPlugin({
  pluginId: id,
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.get('/health', (req, res) => {
          res.json({ ok: true });
        });
        httpRouter.use(router);
      },
    });
  },
});
