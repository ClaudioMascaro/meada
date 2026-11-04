import { Router } from 'express';

export function createRouter() {
  const router = Router();
  router.get('/requests/:id', (req, res) => {
    res.json({ id: req.params.id, plugin: 'approval' });
  });
  return router;
}
