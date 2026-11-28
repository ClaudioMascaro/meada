# Módulos Terraform mapeados

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Interpreta llamadas a módulos conocidos o mapeados por el repositorio como recursos aprovisionados
y cargas de trabajo.

## Interfaz

`mapped_module` evalúa la entrada en la instancia que hizo la llamada. `module_workload` usa el
mapeo para seleccionar Lambda o ECS.
[modules.rs:mapped_module](./modules.rs) [modules.rs:module_workload](./modules.rs)

## Invariantes

Las condiciones de creación deben permitir el recurso. Un nombre generado por prefijo impide
identificar el nombre físico aprovisionado.
[modules.rs:module_created](./modules.rs) [modules.rs:mapped_module](./modules.rs)

Un argumento booleano omitido toma el valor predeterminado del módulo. Si el argumento está
presente pero no se puede evaluar como booleano, la condición queda desconocida.
[modules.rs:module_bool](./modules.rs)

## Efectos secundarios

El evaluador actualiza la proyección del nodo, su nombre o patrón y los orígenes que pueden
sobrescribirlo, dentro del presupuesto de evaluación.
[modules.rs:mapped_module](./modules.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- custom_iac::
```
