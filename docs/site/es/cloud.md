# Módulos de nube

Módulos opt-in, desactivados por defecto y documentados abajo. Un módulo
es una feature de cargo más una sección de `meada.workspace.toml`; los binarios de release son el
build por defecto, sin ninguno, y una sección en un build sin su feature es un error que nombra la
feature. La feature `cloud` los activa todos a la vez (`cargo install meada --features cloud`) y,
como `refresh-lock`, solo compila en Unix.

Elige los módulos según tu operación. Cada uno es opt-in: requiere la feature del binario y su sección en el manifiesto.

- [Snapshots](snapshots.md)
- [Generaciones y retención](generations.md)
- [Refresh y publicación](refresh.md)
- [Autenticación y grants](admission.md)
- [Credenciales de GitHub App](github-app.md)
- [Consola de administración](console.md)
