// Names bound here by a parameter, a local or nothing at all: no use links, though other files
// define functions of the same names.
export function show(handler: string, t: number): Promise<Record<string, number>> {
  console.log(t);
  const m = handler;
  return Promise.resolve({ [m]: t });
}

export function render() {
  return <div>{Header}</div>;
}
