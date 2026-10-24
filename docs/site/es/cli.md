# Uso

En la raíz del repositorio:

- `meada index` arma el índice, en `.meada/`, del repositorio y de los pares;
- `meada status` muestra el conjunto de repositorios, el estado de cada índice y los pares que
  no se encontraron; bajo cada repositorio, los recursos sin enlace y los `via` que no
  corresponden a ningún símbolo;
- `meada export` imprime todo el índice como JSON ordenado, útil para comparar dos ejecuciones
  con `diff`;
- `meada decide` resuelve enlaces ambiguos y URLs dinámicas, ver Decisiones;
- `meada mcp` sirve las [herramientas MCP](mcp.md).

Meada solo escribe dentro de `.meada/`, que se ignora a sí mismo en git. Toda consulta primero
actualiza el índice con lo que cambió en los archivos, incluso lo que aún no se hizo commit. Los
mensajes de la línea de comandos siguen `LANG` (portugués o inglés); los del MCP son siempre en
inglés.
