import { coreServices, createBackendModule } from '@backstage/backend-plugin-api';
import { Router } from 'express';

// A module (#813) gets the httpRouter of the plugin it extends: its routes sit under that
// plugin's mount, `/api/catalog`.
export const catalogModuleOrdersMetadata = createBackendModule({
  pluginId: 'catalog',
  moduleId: 'orders-metadata',
  register(reg) {
    reg.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        const router = Router();
        router.put('/orders-metadata/v1/metadata', (req, res) => {
          res.json({});
        });
        httpRouter.use(router);
      },
    });
  },
});
