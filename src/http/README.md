# Extração de contratos HTTP

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Reúne rotas servidas, chamadas consumidoras e caminhos publicados na infraestrutura sob a
identidade `http::<METHOD>::<path>`.

## Interface

Os adaptadores por linguagem retornam provedores e consumidores pelos fatos de código. O leitor de
IaC declara rotas a partir da infraestrutura avaliada. `prepare` combina Services, Ingresses e
cargas de trabalho para explicar os vínculos.
[../http.rs:Http](../http.rs) [iac.rs:declare](./iac.rs) [infrastructure.rs:prepare](./infrastructure.rs)

Os provedores incluem Express e arquivos `route` do Next.js, FastAPI e Flask, roteadores Go,
Spring MVC/JAX-RS, Ktor, controladores ASP.NET, actix-web/Rocket e Vapor/Hummingbird.
[ts.rs:providers](./ts.rs) [python.rs:providers](./python.rs) [go.rs:providers](./go.rs) [java.rs:providers](./java.rs) [kotlin.rs:providers](./kotlin.rs) [csharp.rs:providers](./csharp.rs) [rust.rs:providers](./rust.rs) [swift.rs:providers](./swift.rs)

- [Rotas e clientes HTTP em C#](./csharp/README.md)
- [Clientes HTTP em Go](./go/README.md)
- [Clientes HTTP da JVM](./java/README.md)
- [Clientes HTTP Ktor](./kotlin/README.md)
- [Clientes HTTP Python e Rust](./python/README.md)
- [Rotas e bibliotecas HTTP em Rust](./rust/README.md)
- [Rotas e clientes HTTP em Swift](./swift/README.md)
- [Rotas e consumidores JavaScript e TypeScript](./ts/README.md)

## Invariantes

A identidade usa o método e o caminho normalizado. URLs relativas são resolvidas pelas regras de
referências RFC 3986 onde o cliente as adota. O vínculo por infraestrutura depende da estrutura de
rotas avaliada.
[../http.rs:contract](../http.rs) [join.rs:resolve](./join.rs) [infrastructure.rs:prepare](./infrastructure.rs)

Uma variável de `[[base_url]]` restringe os provedores ao diretório declarado no próprio
repositório. Para URL relativa, se o pacote do consumidor servir a rota, a busca conserva apenas
os provedores desse pacote. Um vínculo que depende da reescrita pelo gateway permanece no máximo
`inferred`.
[../http.rs:link](../http.rs) [../http.rs:under](../http.rs)

Se a lista serializada de rotas IaC ultrapassar o orçamento do arquivo, o leitor descarta o resumo
inteiro. Nas URLs vindas do ambiente de uma carga, a preparação reaplica a remoção explícita de
barras antes de concatenar o sufixo escrito pelo cliente.
[iac.rs:IAC_HTTP_STORED_MAX](./iac.rs) [infrastructure.rs:workload_url](./infrastructure.rs)

## Efeitos colaterais

A preparação constrói os vínculos e suas explicações em memória para a consulta.
[infrastructure.rs:Prepared](./infrastructure.rs) [infrastructure.rs:prepare](./infrastructure.rs)

`prepare` também substitui consumidores pelas variantes resolvidas das cargas, ordena e deduplica
os resultados e acrescenta rotas de gateway aos provedores.
[infrastructure.rs:prepare](./infrastructure.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http:: http_iac:: urls:: test_routes:: base_url:: http_bases:: helm_env::
```
