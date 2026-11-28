# Resolução de chamadas e valores

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

O resolvedor combina escopo, classe do receptor e escritas para encontrar alvos de chamadas e
determinar o valor disponível em cada leitura.

## Interface

`value_within_at` avalia o valor no local de leitura. `member_changed` consulta escritas do código
e mutações adicionais reconhecidas pelo leitor da biblioteca.
[mutations.rs:value_within_at](./mutations.rs) [mutations.rs:member_changed](./mutations.rs)

Para TypeScript, o resolvedor seleciona a declaração visível no ponto da chamada e verifica a
classe do receptor e os membros redefinidos nas subclasses. Um método chamado num valor global
conhecido fica sem candidato do repositório. Uma anotação de biblioteca ainda pode admitir métodos
de objetos literais, dublês de teste e classes que declaram o tipo na hierarquia.
[ts.rs:ts_local](./ts.rs) [ts.rs:receiver_class](./ts.rs) [ts.rs:ts_override](./ts.rs) [ts.rs:ts_library_call](./ts.rs)

- [Campos e métodos de objetos em TypeScript](./ts/README.md)

## Invariantes

Uma escrita posterior à leitura pode afetar a próxima iteração do mesmo laço. Um índice incompleto
ou orçamento esgotado impede usar o campo como valor conhecido.
[mutations.rs:looped](./mutations.rs) [mutations.rs:changed](./mutations.rs)

Em Python, ao avaliar um retorno, um decorador fora de `PY_KEEPS_BODY` na função, na classe ou
numa classe derivada que possa substituir o método deixa o resultado desconhecido. Na JVM, um
método privado alcançado por uma chamada exata conserva seu corpo mesmo quando outra classe
declara o mesmo nome.
[../resolve.rs:py_decorated](../resolve.rs) [../resolve.rs:PY_KEEPS_BODY](../resolve.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Efeitos colaterais

A primeira consulta monta um índice em memória para o leitor de mutações. As leituras seguintes
reaproveitam esse índice e debitam o trabalho por arquivo.
[mutations.rs:member_changed](./mutations.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- resolve:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases:: base_url:: xlang::
```
