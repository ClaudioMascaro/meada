# Infraestructura como código

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Descripción general

Convierte declaraciones de infraestructura en nodos y referencias que pueden consultarse mediante
búsqueda, contexto y rastreo.

## Interfaz

Los lectores evalúan Terraform/OpenTofu, Compose, CloudFormation/SAM, Serverless Framework y
GitHub Actions. La tabla incorporada asocia tipos con contratos. La consulta usa los nombres
evaluados para enlazar aprovisionadores y usos.
[json.rs:read](./json.rs) [compose.rs:evaluate](./compose.rs) [cloudformation.rs:evaluate](./cloudformation.rs) [serverless.rs:evaluate](./serverless.rs) [actions.rs:evaluate](./actions.rs) [types.rs:entry](./types.rs) [provision.rs:links](./provision.rs)

Las proyecciones de Azure y GCP usan los tipos de recurso declarados. Los lectores de cargas y
suscripciones evalúan referencias en la instancia correspondiente. El informe de huérfanos
considera las exportaciones visibles en la consulta.
[azure.rs:azure_projection](./azure.rs) [gcp.rs:gcp_projection](./gcp.rs) [workload.rs:workload](./workload.rs) [subscription.rs:prepare_subscriptions](./subscription.rs) [orphans.rs:Scope](./orphans.rs)

- [Lectura de comandos de GitHub Actions](./actions/README.es.md)
- [Rutas REST de API Gateway](./http/README.es.md)
- [Módulos Terraform mapeados](./mapping/README.es.md)
- [Contenedores ECS en Terraform](./workload/README.es.md)

## Invariantes

Cada raíz de Terraform se evalúa en su propio contexto. Las funciones puras registradas solo usan
sus argumentos. Los usos derivados de configuración requieren valores completos y sin censura para
identificar el contrato.
[../iac.rs:Eval](../iac.rs) [functions.rs:declare](./functions.rs) [config_uses.rs:read](./config_uses.rs)

Al enlazar un uso con su aprovisionador, el cálculo respeta el límite de certeza del uso derivado
del entorno. En CloudFormation, una función intrínseca más allá de 32 niveles deja esa parte del
nombre desconocida.
[provision.rs:directed_uses](./provision.rs) [../contract.rs:use_level](../contract.rs) [cloudformation.rs:INTRINSIC_DEPTH_MAX](./cloudformation.rs)

## Efectos secundarios

La actualización persiste nodos y referencias en el índice mediante una transacción SQLite. En
Actions elimina primero las filas anteriores del archivo. Los enlaces de aprovisionamiento se
derivan durante la consulta.
[../iac.rs:insert](../iac.rs) [actions.rs:refresh](./actions.rs) [provision.rs:links](./provision.rs)

## Verificación

```sh
bounded cargo test --locked -p meada --lib --test it -- iac:: custom_iac:: provision:: helm_env::
```
