import { Button, Base, tokens } from '@scope/ui';
import type { Theme } from '@scope/ui';

export function Page(t: Theme) {
  const gap = tokens.gap;
  return <Button onClick={gap} />;
}

export class Panel extends Base {}

export async function load() {
  const { Widget } = await import('@scope/widgets');
  return Widget;
}

export { Card } from './card';
