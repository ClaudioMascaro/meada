import { coreServices, createBackendPlugin } from '@backstage/backend-plugin-api';
import { createRouter } from './router';

export const approvalPlugin = createBackendPlugin({
  pluginId: 'approval',
  register(env) {
    env.registerInit({
      deps: { httpRouter: coreServices.httpRouter },
      async init({ httpRouter }) {
        httpRouter.use(createRouter());
      },
    });
  },
});
