import { coreServices, createBackendModule } from '@backstage/backend-plugin-api';
import Router from 'express-promise-router';

// A module gets the `httpRouter` of the plugin it extends: its router shares `/api/catalog`.
export const catalogModuleOrders = createBackendModule({
  pluginId: 'catalog',
  moduleId: 'orders',
  register(reg) {
    reg.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.put('/orders-metadata/v1/metadata', async (_req, res) => {
          res.json({});
        });
        httpRouter.use(router);
      },
    });
  },
});
