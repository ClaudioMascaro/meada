# Por qué otra herramienta

Quien trabaja con varios servicios sabe que el bug casi nunca vive en un solo repositorio. El
front llama a una ruta que cambió en el back; el worker consume una cola que otro equipo
renombró. De las herramientas de grafo de código que existen hoy, algunas miran un repositorio
a la vez, otras solo conectan repositorios cuando el texto de la ruta coincide exactamente, y
otras prohíben el uso comercial.

Meada quiere seguir el hilo de punta a punta, atravesando todos los repositorios que haga falta,
y decir con qué confianza se hizo cada conexión.

## El nombre

*Meada* es una madeja de hilo. *Puxar o fio da meada*, "tirar del hilo de la madeja", es
encontrar la punta que deshace el nudo, y eso es lo que intenta hacer la herramienta cuando
sigue una llamada de un repositorio a otro.

## Licencia

[Apache-2.0](https://github.com/ClaudioMascaro/meada/blob/main/LICENSE).
