# Valores HCL y selectores de configuración

Valores de configuración en HCL y resolución de ubicaciones de configuración citadas por declaraciones.

## Interfaz

El lector HCL convierte atributos en filas con ruta de clave y ubicación. El resolvedor recibe un selector y busca los sitios correspondientes en el índice del repositorio. `[hcl.rs:read]` `[sites.rs:locate]` `[sites.rs:Site]`

## Invariantes

Se omiten los valores nulos. Los selectores exigen valores escalares completos y descartan textos truncados u ocultos. `[hcl.rs:read]` `[sites.rs:locate]`

El lector HCL rechaza el archivo cuando el anidamiento de llaves, corchetes y paréntesis supera los 64 niveles. `[hcl.rs:NESTING_MAX]`

## Efectos secundarios

El analizador trabaja sobre los bytes suministrados. La localización consulta la conexión SQLite que abrió el llamador. `[hcl.rs:read]` `[sites.rs:locate]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it config_values::`.
