# Objetos Kubernetes e recursos relacionados

Fatos Kubernetes para composição Crossplane, recursos personalizados, topologia RabbitMQ, Ingress e ambiente de contêineres.

## Interface

Os leitores extraem fatos de objetos, identificam referências e preparam os nomes físicos e alvos que os tipos de contrato consultam. `[crossplane.rs:facts]` `[custom.rs:physical]` `[topology.rs:Prepared]` `[routing.rs:read]` `[environment.rs:resolve]`

## Invariantes

A resolução de ambiente busca objetos por namespace, tipo e nome dentro do conjunto recebido. O Ingress só atribui um Service a um caminho `Prefix` literal e sem anotações de reescrita ou expressão regular. `[environment.rs:configs]` `[environment.rs:resolve]` `[routing.rs:read]`

Uma composição Crossplane pode contribuir com até 64 recursos compostos. Os que excederem esse total ficam fora do índice. `[crossplane.rs:COMPOSED_MAX]`

## Efeitos colaterais

O processamento usa objetos já lidos e transforma referências de ambiente em memória ao aplicar prefixos, sufixos e namespace do overlay. `[environment.rs:transform]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it kubernetes::`.
