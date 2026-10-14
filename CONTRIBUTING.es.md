# Cómo contribuir a Meada

[Português (Brasil)](CONTRIBUTING.md) · [English](CONTRIBUTING.en.md) · **Español**

> Esta es una traducción. La [guía](CONTRIBUTING.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

Gracias por querer ayudar. Esta guía explica cómo preparar el entorno, cómo proponer un cambio y
qué espera el proyecto de ti. Al participar, aceptas seguir el
[Código de Conducta](CODE_OF_CONDUCT.es.md).

## En qué idioma escribir

- **Issues, PRs y revisiones** pueden escribirse en portugués, español o inglés. Respondemos en
  el idioma que usaste.
- **Código, identificadores, comentarios, mensajes de commit y ADRs** van en inglés, para que
  cualquier persona pueda leerlos.
- **La documentación para personas** (el README, esta guía, el código de conducta) nace en
  portugués de Brasil. Los archivos `.en.md` y `.es.md` son traducciones, en ese orden de
  prioridad. Si cambias un archivo en portugués, actualiza las dos traducciones en el mismo PR;
  si no puedes, abre un issue con la etiqueta `translation` por cada una que quede atrás.
- ¿Algún pasaje del inglés o del español suena raro? Corregir una traducción es una excelente
  primera contribución.

## Antes de empezar

- Busca un issue abierto que ya cubra tu idea. Si no hay, abre uno. Para cambios grandes,
  conversa en el issue antes de escribir código.
- Los issues con la etiqueta `good first issue` están pensados para quien llega, y `help wanted`
  marca dónde hace más falta ayuda.
- El alcance de la v1 y el orden del trabajo están en el
  [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md).
- Usa los términos de [`GLOSSARY.md`](GLOSSARY.md) en el código, los docs y los issues:
  *repository*, *index*, *peer*, *provider*, *consumer*, *contract*, *link*, *level*, *chain* y
  los demás.
- Las decisiones del proyecto están en [`docs/adr`](docs/adr). Un cambio que contradice alguna
  empieza con un nuevo ADR. En particular, los invariantes del producto (sin LLM ni red en la
  indexación, local primero, sin herramientas de escritura en MCP, un índice por repositorio,
  determinismo) están en el [ADR 0002](docs/adr/0002-product-invariants.md).

## Regla de clean room

Meada cubre un terreno que GitNexus (licencia PolyForm Noncommercial) también cubre. Para no
contaminar el proyecto:

- construye a partir de los ADRs y las specs del propio Meada;
- no copies ni leas el código fuente de GitNexus, el `dist/` de su paquete npm ni sus constantes
  ajustadas, y no pegues nada de eso en issues, PRs o commits.

## Preparar el entorno

Necesitas Rust. El archivo `rust-toolchain.toml` fija la versión del compilador e instala
`rustfmt` y `clippy` por sí solo cuando usas [rustup](https://rustup.rs).

```sh
git clone https://github.com/ClaudioMascaro/meada.git
cd meada
cargo build
cargo test
```

## Antes de abrir el PR

Estos tres comandos son los mismos que corre la CI (`fmt`, `clippy`, `test`), y los tres deben
pasar:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Si el cambio toca el comportamiento, incluye la prueba que lo cubre.

## Commits y PRs

- **Conventional Commits**: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`, con
  alcance opcional. Ejemplo: `fix(resolve): keep the link level when a route has a prefix`.
- Nombra la rama según el tipo: `feat/...`, `fix/...`, `docs/...`.
- Abre el PR contra `main` y completa la plantilla. Vincula el PR al issue (`Closes #123`).
- El PR se integra con **squash merge**: la rama se convierte en un único commit en `main`, y el
  título del PR pasa a ser el mensaje de ese commit. Por eso el título del PR también sigue
  Conventional Commits.
- Los checks `fmt`, `clippy` y `test` deben estar en verde antes de integrar.
- Mantén el PR pequeño y con un solo propósito. Los cambios sin relación van en PRs separados.

## Seguridad

No abras un issue público para una vulnerabilidad. Consulta la
[política de seguridad](SECURITY.es.md).
