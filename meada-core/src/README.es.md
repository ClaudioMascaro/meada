# meada-core · Rust

Tipos y operaciones sobre manifiestos, identidades, permisos y exportaciones.

## Interfaz

Valida entradas TOML, compara versiones, autoriza cambios e interpreta exportaciones del índice. La feature `wasm-bindgen` expone estas operaciones al navegador. `[manifest.rs:validate]` `[workspace.rs:diff]` `[authorize.rs:authorize_entries]` `[export.rs:Export]` `[lib.rs:wasm]`

El manifiesto también describe la base HTTP por variable de entorno (`[[base_url]]`), la ruta del entorno del contenedor en los valores de Helm (`[[helm_env]]`) y la llamada del cliente de canal que indica el tópico (`[[channel_client]]`). `[manifest.rs:BaseUrl]` `[manifest.rs:HelmEnv]` `[manifest.rs:ChannelClient]`

## Invariantes

El navegador usa la validación de importación, que admite otras secciones del manifiesto al validar las entradas. El campo `owner` admite hasta 256 caracteres. La autorización recibe el rol, la concesión y las entradas como datos. `[wasm.rs:validate]` `[manifest.rs:validate_import]` `[manifest.rs:OWNER_MAX]` `[authorize.rs:authorize_entries]`

## Efectos secundarios

La inicialización del módulo WASM selecciona el idioma de los mensajes. El llamador proporciona el texto de manifiestos y exportaciones. `[wasm.rs:init]`

## Verificación

Desde la raíz del repositorio, ejecuta `cargo test -p meada-core --features wasm-bindgen`.
