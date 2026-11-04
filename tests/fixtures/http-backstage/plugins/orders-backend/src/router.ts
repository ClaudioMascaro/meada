import { Router } from 'express';

export async function createRouter() {
  const router = Router();
  router.get('/requests/:id', (req, res) => {
    res.json({ id: req.params.id, plugin: 'orders' });
  });
  return router;
}
