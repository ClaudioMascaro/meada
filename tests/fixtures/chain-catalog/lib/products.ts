// A cycle: load and parent call each other before `find` ends the chain.
export function load(id: string, depth: number): unknown {
  return depth > 0 ? parent(id, depth) : find(id);
}

function parent(id: string, depth: number) {
  return load(`${id}-parent`, depth - 1);
}

function find(id: string) {
  return { id };
}
