import type { Repo } from '@/services/repo';
export function alias(r: Repo) { r.load(); }
