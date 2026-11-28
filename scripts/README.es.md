# Herramientas de mantenimiento

Comprobación local de la rama, validación de perfiles de despliegue y herramientas que usan las pruebas.

## Interfaz

`scripts/check` selecciona las comprobaciones según el alcance del cambio. El oráculo de expresiones regulares recibe un vector JSON y comprueba cada patrón con el motor de Go. `[check:main]` `[regexp-oracle.go:main]`

## Invariantes

El oráculo devuelve dos resultados por patrón en el orden de entrada. Una compilación correcta produce una cadena vacía. `[regexp-oracle.go:main]`

## Efectos secundarios

El verificador ejecuta procesos de compilación y prueba. El oráculo escribe en stdout o en el archivo del segundo argumento. `[check:bounded]` `[regexp-oracle.go:main]`

## Componentes

- [Compilación del sitio](./docs/README.es.md)

## Verificación

Desde la raíz del repositorio, ejecuta `scripts/check --selftest`.
