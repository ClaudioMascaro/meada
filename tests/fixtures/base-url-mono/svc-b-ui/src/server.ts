import express from 'express';

const app = express();
const router = express.Router();
router.get('/status', (req, res) => res.json({ ok: true }));
app.use(router);
app.listen(3000);
