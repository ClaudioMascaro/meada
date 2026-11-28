# Chamadas aos SDKs do Azure por linguagem

Formas de chamada dos SDKs do Azure em C#, Go, Java, Python e JS/TS.

## Interface

Cada leitor reconhece construtores importados, segue clientes derivados e classifica a operação final. O leitor de ambiente combina as partes da identidade do receptor. `[csharp.rs:constructed]` `[go.rs:step]` `[java.rs:operation]` `[python.rs:read]` `[ts.rs:read]` `[environment.rs:environment_identity]`

## Invariantes

Quando o tópico e a assinatura do Service Bus vêm de variáveis de ambiente, seus nomes precisam ser inequívocos no mesmo escopo de execução. Literais podem ser usados diretamente. `[environment.rs:environment_identity]`

Para combinar valores do mesmo escopo, o texto deve ser não vazio e todas as ocorrências devem concordar também sobre se o valor é uma referência tipada. `[environment.rs:unique]`

## Efeitos colaterais

A análise trabalha sobre chamadas e valores já coletados, em memória. `[ts.rs:read]` `[environment.rs:environment_identity]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it azure_sdk::`.
