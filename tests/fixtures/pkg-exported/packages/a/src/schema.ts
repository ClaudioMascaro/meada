import { z } from 'zod';

export type Foo = z.infer<typeof Foo>;
export const Foo = z.object({ id: z.string(), size: z.number() });
