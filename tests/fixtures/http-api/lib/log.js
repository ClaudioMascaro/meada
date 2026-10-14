export function withLogging(handler) {
  return (...args) => handler(...args);
}
