# Recursos usados pelo código ou declarados na configuração

Contratos de recursos a partir de chamadas aos SDKs da AWS e de localizações declaradas em configuração indexada.

## Interface

A tabela de operações informa o serviço, a chave de nome e a direção. Leitores de boto3 e JS/TS identificam o cliente e os argumentos. O leitor de declarações resolve seletores de configuração em locais do índice. `[aws.rs:OPERATIONS]` `[aws.rs:extract]` `[python.rs:sides]` `[ts.rs:sides]` `[via.rs:read]`

A consulta inversa por local mostra os recursos que o local fornece ou consome, os leitores ou escritores correspondentes e a evidência de cada vínculo. `[via.rs:context]`

## Invariantes

Em JS/TS, cliente e comando devem vir do mesmo pacote do SDK previsto para a operação. Em Python, a fábrica do cliente deve corresponder ao serviço. `[ts.rs:sides]` `[python.rs:sides]`

Ao ligar um leitor a um escritor, o vínculo preserva o nível de evidência mais fraco dos dois lados. Um nome obtido do ambiente de execução limita o vínculo a `inferred` quando o lado não registra outro limite. `[../resource.rs:link]` `[../contract.rs:environment_cap]`

## Efeitos colaterais

A resolução de declarações consulta o SQLite e acrescenta os locais encontrados à exportação em memória. A análise do SDK trabalha com fatos previamente extraídos. `[via.rs:read]` `[aws.rs:extract]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it resources::`.
