# Proyectos y configuración .NET

Lectores compartidos de proyectos MSBuild, referencias entre proyectos y configuración .NET.

## Interfaz

El lector extrae propiedades, usings y referencias de proyectos SDK y de los archivos `Directory.Build.props` suministrados. La configuración resuelve claves como `A:B` en el `appsettings.json` del proyecto. `[msbuild.rs:read]` `[msbuild.rs:chain]` `[references.rs:referenced]` `[appsettings.rs:Settings]`

## Invariantes

La búsqueda de configuración compara los segmentos sin distinguir mayúsculas y minúsculas y solo acepta cadenas. El recorrido de referencias limita la profundidad. `[appsettings.rs:get]` `[references.rs:REFERENCE_DEPTH]`

La cadena de importaciones de `Directory.Build.props` incluye hasta ocho archivos. `[msbuild.rs:chain]` `[msbuild.rs:PROPS_DEPTH]`

## Efectos secundarios

Los lectores usan los bytes y los proyectos suministrados por el llamador sin iniciar MSBuild ni el SDK de .NET. `[msbuild.rs:read]` `[references.rs:referenced]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it nugetpkg::`.
