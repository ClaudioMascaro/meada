# Rotas e consumidores JavaScript e TypeScript

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Relaciona chamadas a `fetch` com rotas e com a configuração de plugins e proxies do Backstage.

## Interface

`consumers` lê URL, método e campos acessados na resposta. `plugins` e `discovered` identificam a
montagem e a descoberta de plugins. `Config` resolve leituras de configuração e destinos de proxy.
[clients.rs:consumers](./clients.rs) [backstage.rs:plugins](./backstage.rs) [backstage.rs:discovered](./backstage.rs) [config.rs:Config](./config.rs)

`Config` lê cada base distinta de `app-config*.yaml` e de suas inclusões. Uma chave dinâmica de um
objeto pode produzir vários destinos. A expansão de `fetch` preserva o local da requisição quando
um método auxiliar devolve a base de descoberta. `pluginId` também pode vir de uma constante do
mesmo arquivo em `createBackendPlugin` ou `createBackendModule`.
[config.rs:decided](./config.rs) [backstage.rs:discovered](./backstage.rs) [backstage.rs:plugins](./backstage.rs) [clients.rs:consumers](./clients.rs)

`Config::literals` devolve os valores distintos de uma chave em ordem determinística. `whole`
informa se todas as raízes e inclusões puderam ser lidas. Um valor inteiro `${VAR}` mantém o nome
da variável para resolver a base com a configuração da carga.
[config.rs:literals](./config.rs) [config.rs:whole](./config.rs) [config.rs:configured](./config.rs)

- [Inclusões de configuração Backstage](./config/README.md)

## Invariantes

`fetch` só conta onde o pacote de linguagem o declara global, incluindo parâmetros com `fetch`
como valor padrão. Um plugin com identificador literal tem a montagem `/api/<id>`.
[clients.rs:consumers](./clients.rs) [backstage.rs:mount](./backstage.rs)

Várias bases, configuração incompleta ou seleção dinâmica de um registro limitam os vínculos a
`inferred`. Se o destino do proxy não puder ser lido, o consumidor conserva o nome do endpoint e
indica a falha na leitura do destino.
[config.rs:decided](./config.rs) [config.rs:configured](./config.rs) [clients.rs:consumers](./clients.rs) [backstage.rs:proxied](./backstage.rs)

## Efeitos colaterais

A configuração é analisada sob demanda. As inclusões que nenhuma raiz seguiu viram registros com a
razão da recusa.
[config.rs:Config](./config.rs) [config.rs:unfollowed](./config.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- http::ts urls:: http_bases::
```
