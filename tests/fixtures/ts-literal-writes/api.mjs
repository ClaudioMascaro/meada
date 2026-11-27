const api = { list: () => 'orders' };

export function store(key) {
  const values = {};
  values[key] = 'a';
  return values;
}

export function read() { return api.list; }
export function call() { return api.list(); }

export { api };
