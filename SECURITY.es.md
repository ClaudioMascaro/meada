# Política de seguridad

[Português (Brasil)](SECURITY.md) · [English](SECURITY.en.md) · **Español**

> Esta es una traducción. La [política](SECURITY.md) en portugués de Brasil es la fuente; si no
> coinciden, vale el original, y cualquier corrección a esta página es bienvenida.

## Versiones con soporte

Solo la **última versión 1.x publicada** recibe correcciones de seguridad. Actualiza a la
última versión antes de reportar, si puedes.

## Cómo reportar una vulnerabilidad

**No abras un issue público.** Usa el reporte privado de vulnerabilidades de GitHub:

1. Abre la pestaña [Security](https://github.com/ClaudioMascaro/meada/security) del
   repositorio.
2. Elige **Report a vulnerability**.
3. Describe el problema: la versión de Meada, el sistema operativo, los pasos para reproducirlo
   y el impacto que ves.

Puedes escribir en portugués, español o inglés.

## Qué pasa después

El mantenedor confirma la recepción, evalúa el reporte y conversa contigo a través del propio
reporte privado. Cuando haya una corrección, sale en una nueva versión 1.x (patch o minor), y el aviso de
seguridad de GitHub da el crédito a quien reportó, si lo deseas.

## Qué interesa

Por ejemplo: que la indexación o el servidor MCP escriban fuera del índice propio de Meada,
lean archivos fuera del repositorio indexado, ejecuten código que viene del repositorio
indexado, o expongan el servidor MCP más allá de la máquina local. Si dudas de que algo sea una
vulnerabilidad, repórtalo igual.
