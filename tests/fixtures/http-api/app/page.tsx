// A page, not a route handler: it serves no HTTP contract.
export default function Home() {
  return <main>api</main>;
}

export function GET() {
  return null;
}
