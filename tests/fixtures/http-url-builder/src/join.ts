const API = "https://api.example.com";
declare const settings: { host: string };

// exact: `new URL(reference, base)` is the join of the two (WHATWG URL Standard, which agrees
// with RFC 3986 section 5.2 for these references). An absolute reference is itself.
export function absolute() {
  return fetch(new URL("https://api.example.com/api/vets", "https://other.invalid/x"));
}

// exact: `//host/path` takes its host from the reference.
export function networkPath() {
  return fetch(new URL("//api.example.com/api/owners", "https://other.invalid/x"));
}

// exact: `/path` replaces the base's path.
export function absolutePath() {
  return fetch(new URL("/api/vets", `${API}/old/path?page=1`));
}

// exact: a relative reference joins the base's directory, so a trailing `/` keeps the last segment.
export function relativeUnderSlash() {
  return fetch(new URL("owners", `${API}/api/`));
}

// exact: without it the last segment goes.
export function relativeWithoutSlash() {
  return fetch(new URL("api/vets", `${API}/old`));
}

// exact: a constant holds the join.
export function throughConstant() {
  const u = new URL("/api/owners", API);
  return fetch(u);
}

// exact: a client wrapper joins its parameter at each call site, whichever side it is on.
function get(path: string) {
  return fetch(new URL(path, API));
}

function from(base: string) {
  return fetch(new URL("/api/vets", base));
}

export function wrapped() {
  get("/api/owners");
  return from(API);
}

// An environment read as the base: an absolute-path reference keeps the opaque lead, so the
// contract is read and the host is the variable's (`via env:API_URL`).
export function envAbsolutePath() {
  return fetch(new URL("/api/vets", process.env.API_URL));
}

// unresolved: a relative reference needs the base's path, and a reference that starts with a read
// cannot say whether it is a scheme, a path or a host; a query or a fragment alone keep the
// base's path, which a literal base gives and the read does not here.
export function envRelative() {
  return fetch(new URL("api/vets", process.env.API_URL));
}

export function envReference() {
  return fetch(new URL(`${process.env.PREFIX}/api/vets`, API));
}

export function queryOnly() {
  return fetch(new URL("?page=1", API));
}

export function fragmentOnly() {
  return fetch(new URL("#top", API));
}

// A base that is a hole (a member the file does not fold) keeps the lead too: the contract is
// read and the host stays unresolved.
export function holeAbsolutePath() {
  return fetch(new URL("/api/vets", `${settings.host}`));
}

// The same for a base written bare, a member or a call the file does not fold.
export function holeBareAbsolutePath() {
  return fetch(new URL("/api/owners", settings.host));
}

// unresolved: a relative reference needs the hole's path.
export function holeBareRelative() {
  return fetch(new URL("api/owners", settings.host));
}

// unresolved: `//host/x` needs the base's scheme, which an environment read does not give.
export function envNetworkPath() {
  return fetch(new URL("//api.example.com/api/vets", process.env.API_URL));
}
