// `types` maps through the `outDir` and comes first, so it wins over a `main` that is a source
// (`prec`). A nested package's file is never the default `./index` or a subpath of the outer
// package (`inner`, `deep`).
import { prec } from '@example/prec';
import { inner } from '@example/nest';
import { deep } from '@example/nest/sub/deep';

export function nest(): void {
  prec();
  inner();
  deep();
}
