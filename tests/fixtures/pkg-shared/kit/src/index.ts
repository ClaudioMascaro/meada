// The entry `exports` names: a re-export, every name of another file, and a default.
export { greet } from './greet';
export * from './more';

export default function kit(): string {
  return 'kit';
}
