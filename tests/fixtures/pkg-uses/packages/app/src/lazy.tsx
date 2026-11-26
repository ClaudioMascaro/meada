import { lazy } from 'react';

export const Lazy = lazy(() => import('@scope/a').then((m) => ({ default: m.Page })));
