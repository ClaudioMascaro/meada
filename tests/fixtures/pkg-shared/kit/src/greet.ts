export function greet(name: string): string {
  return shout(`hello ${name}`);
}

function shout(text: string): string {
  return text.toUpperCase();
}
