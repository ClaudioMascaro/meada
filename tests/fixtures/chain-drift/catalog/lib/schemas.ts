import { z } from "zod";

// The contract of a comment: there is no `rating` any more. `meta` is a record, so open.
export const Comment = z.object({
  id: z.string(),
  title: z.string().min(1),
  tags: z.array(z.string()).optional(),
  author: z.object({ name: z.string() }).nullish(),
  meta: z.record(z.string()),
});

export const Comments = z.array(Comment);

export const Summary = z.object({ count: z.number(), average: z.number() });
