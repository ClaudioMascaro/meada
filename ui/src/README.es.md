# Aplicación de la consola

Páginas React, cliente HTTP e integración con el núcleo WASM.

## Interfaz

El arranque carga la configuración, elige el modo de autenticación y pasa el cliente de la API a la aplicación. `[main.tsx:start]` `[App.tsx:App]` `[api.ts:Api]`

## Invariantes

El callback de autenticación quita el código de la URL antes del descubrimiento y el intercambio. Una discrepancia de origen provoca una redirección en el flujo con emisor. `[main.tsx:start]` El callback consume el verificador PKCE y exige que el parámetro `state` coincida con el guardado en la pestaña. `[auth.ts:callback]`

## Efectos secundarios

La interfaz envía solicitudes HTTP, usa el almacenamiento de sesión de la pestaña y muestra los resultados en el DOM. `[api.ts:Api]` `[main.tsx:start]` `[local.ts:LocalAuth]`

## Verificación

Desde la raíz del repositorio, ejecuta `npm --prefix ui run typecheck && npm --prefix ui test`.
