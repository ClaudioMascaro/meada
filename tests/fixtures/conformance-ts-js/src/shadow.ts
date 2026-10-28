// Hidden names: in each function below a binding spells the top-level `helper`, so the call
// after it never reaches that definition exactly. A `let` or `var` holding a function is a
// definition of its own unless it is assigned again, as it is here.
export function helper(): number {
  return 1;
}

export function plain(): number {
  return helper();
}

export function caught(): void {
  try {
    throw new Error('x');
  } catch (helper) {
    helper();
  }
}

export function looped(items: Array<() => void>): void {
  for (const helper of items) helper();
}

export function rebound(): void {
  let helper = () => 2;
  helper = () => 5;
  helper();
}

export function hoisted(): void {
  var helper = () => 3;
  helper = () => 6;
  helper();
}

export function unparenthesized(): object {
  const o = { run: helper => helper() };
  return o;
}

export function nested({ a: { helper } }: { a: { helper: () => void } }): void {
  helper();
}

export function listed([helper]: Array<() => void>): void {
  helper();
}

// A constant of the same function is hidden too, and so is a parameter.
export function twice(): void {
  const helper = 'not called';
  for (const helper of [() => 4]) helper();
}

export function again(helper: () => void): void {
  try {
    throw new Error('x');
  } catch (helper) {
    helper();
  }
}
