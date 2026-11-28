# Alertas y reenvío de incidentes

Lectura estática de reglas y rutas de monitorización, configuración de incidentes y reenvíos de receptores de Alertmanager.

## Interfaz

El lector resume las reglas, rutas y receptores de los archivos de configuración. A partir de una ruta y una regla, el reenvío produce eventos para receptores conocidos. `[config.rs:read]` `[incident.rs:terraform]` `[forwarding.rs:events]`

## Invariantes

Las expresiones regulares respetan el subconjunto admitido de Go/RE2 y los límites de coste. Una condición PCL ajena al subconjunto literal queda como desconocida. `[regexp.rs:compile]` `[pcl.rs:parse]`

Una condición PCL con más de 64 niveles de grupos u operadores unarios queda sin evaluar. `[pcl.rs:ALERT_PCL_DEPTH_MAX]`

## Efectos secundarios

El análisis produce resúmenes y eventos en memoria. El reenvío sustituye las plantillas admitidas sin ejecutarlas ni enviar notificaciones. `[config.rs:Summary]` `[forwarding.rs:events]`

## Verificación

Desde la raíz del repositorio, ejecute `cargo test -p meada --test it alerts::`.
