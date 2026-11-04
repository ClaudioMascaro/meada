import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { createRouter } from './router';

export const ordersPlugin = createBackendPlugin({
  pluginId: 'orders',
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        httpRouter.use(await createRouter());
      },
    });
  },
});
