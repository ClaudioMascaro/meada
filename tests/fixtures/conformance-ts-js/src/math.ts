export function add(a: number, b: number): number {
  return a + b;
}

export const double = (n: number): number => add(n, n);

export const square = function (n: number): number {
  return n * n;
};

export default function scale(n: number, by: number): number {
  function times(): number {
    return n * by;
  }
  return double(times());
}
