export function add(a: number, b: number): number {
  return a + b;
}

export function sum(values: number[]): number {
  let total = 0;
  for (const v of values) {
    total = add(total, v);
  }
  return add(total, 0);
}

function outer(): number {
  function inner(): number {
    return add(1, 2);
  }
  return inner() + inner();
}

console.log(outer());
