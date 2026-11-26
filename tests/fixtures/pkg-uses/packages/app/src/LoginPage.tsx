import { Button, Base, tokens, Mode } from '@scope/ui';
import type { Theme } from '@scope/ui';
import { Kind } from './kinds';

function Header() {
  return null;
}

function handler() {}

export function LoginPage(t: Theme) {
  const m: Mode = Mode.A;
  const k: Kind = Kind.A;
  return (
    <div>
      <Header />
      <Button onClick={handler} />
    </div>
  );
}

export class Page extends Base {}

export const gap = tokens.gap;

export const routes = [<LoginPage />];

function both() {
  Button();
  return <Button />;
}

export const Foo = 1;
export type Foo = number;
export function useFoo(f: Foo) {
  return Foo;
}

enum Local {
  A,
}

export function useLocal() {
  return Local.A;
}
