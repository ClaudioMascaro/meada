import { Router } from 'express';

export async function createRouter() {
  const router = Router();
  router.get('/list', (_req, res) => res.json([]));
  router.get('/list/:id', (_req, res) => res.json({}));
  router.put('/list/:id', (_req, res) => res.json({}));
  return router;
}
