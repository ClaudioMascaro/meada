# Identidad empresarial

!!! warning "Planificado: S4"
    [S4, #319](https://github.com/ClaudioMascaro/meada/issues/319) especifica automatización de identidad, proveedores de identidad, proveedores de grant y capa de datos. Ninguna configuración o comando nuevo de esta propuesta debe tratarse como disponible. La [admisión OIDC mediante claims](admission.md) ya está entregada.

## Autenticar, conceder acceso y registrar

El proveedor de identidad seguirá emitiendo el token. El grant efectivo reunirá las identidades permitidas por claims y, cuando se configure, por permisos de GitHub. El job de refresh consultará los roles de GitHub, nunca la consola ni la indexación. Los owners y roles humanos no ampliarán ese grant.

Postgres será un módulo opt-in para estado mutable duradero: auditoría, directorio y permisos. El snapshot store conservará documentos y bundles inmutables; Valkey mantendrá punteros, locks, fences y cachés. `meada index` seguirá sin red.

## Directorio y revocación

SCIM alimentará el directorio de usuarios y grupos, incluidos los grupos que exceden el límite de claims de Entra. Eliminar un sujeto provisionado dejará un bloqueo duradero. Un sujeto nunca provisionado conservará la admisión normal: la ausencia del directorio por sí sola no significará eliminación.

La auditoría deberá registrar el intento antes de la mutación. Si ese registro falla, se rechazará la escritura. Los detalles de caché, expiración, indisponibilidad y revocación pertenecen a la spec S4; no son garantías del runtime entregado.

<!-- meada-graph:scim-revocation -->

## Proveedores de identidad, grants y datos

Las líneas discontinuas representan las integraciones planificadas. El proveedor de identidad emite el token; el job mantiene los permisos; consola y nodos consultan los datos necesarios para la admisión. El directorio y la auditoría guardan estado duradero separado del contenido de código.

<!-- meada-graph:enterprise -->
