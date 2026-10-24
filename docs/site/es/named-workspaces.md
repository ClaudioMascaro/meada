# Workspaces con nombre

!!! warning "Planificado: S5"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) es alcance secuencial aceptado después de S3 y S4. La [consolidación de contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), definirá los contratos finales del runtime. La consola entregada aún opera con un workspace por deployment.

## Componer sin duplicar la indexación

La propuesta permite workspaces con nombre y repositorios superpuestos, sobre indexación compartida. El mismo repositorio no obtiene otro índice por pertenecer a dos workspaces. Habrá tres roles humanos: administrador de la plataforma, operador de workspace y viewer.

El grant efectivo por repositorio seguirá siendo la autoridad. Un operador con acceso solo a A podrá componer Y+A; otro con acceso solo a B podrá componer Y+B. Compartir Y no concederá A al segundo ni B al primero. Leer Y también dependerá del grant efectivo de cada sujeto.

Los miembros del mismo grupo heredarán el rol viewer sin otra aprobación del proveedor de identidad, de Meada o de un paquete de acceso. Ese rol seguirá limitado por los grants de repositorio.

<!-- meada-graph:workspace-overlap -->

## Autoría asistida por MCP

La autoría autenticada por MCP será stateless y producirá únicamente propuestas. No guardará ni aplicará configuraciones. Una persona revisará la propuesta y usará el flujo autorizado de la consola para persistir y aplicar cambios.

La distinción entre [versión de workspace y generación](workspaces.md), y entre [admisión y grant](admission.md), sigue siendo esencial para esta propuesta.

<!-- meada-graph:mcp-proposal -->
