# Filas e tópicos no código

Extração de produtores, consumidores, publicações e vínculos de filas ou tópicos a partir de código de aplicação.

## Interface

Os leitores de C#, Go, Java, Python, Rust e JS/TS convertem chamadas de bibliotecas em mensagens. As operações AWS compartilham uma tabela e os nomes de destino passam pelos mesmos leitores de argumentos. `[csharp.rs:messages]` `[go.rs:messages]` `[java.rs:messages]` `[python.rs:messages]` `[rust.rs:messages]` `[ts.rs:messages]` `[aws.rs:OPERATIONS]` `[message.rs:Message]`

Em Go e JS/TS, `[[channel_client]]` identifica uma função ou método do repositório e indica qual argumento ou campo contém o tópico Kafka. Em Go, também pode identificar uma função pelo caminho de importação. Uma declaração que não produz nenhum lado aparece em `status`. `[go.rs:declared]` `[go.rs:client_sides]` `[ts.rs:declared_sides]` `[../channel.rs:status]`

## Invariantes

Cada linguagem exige evidência da biblioteca ou do receptor previsto. Os leitores de publicação e vínculo AMQP mantêm separados exchange, chave de roteamento e fila. `[python.rs:messages]` `[ts.rs:messages]` `[message.rs:publish]` `[message.rs:bind]`

O leitor KafkaJS reconhece clientes em campos privados e consulta valores literais de `app-config` para uma leitura direta da chave do tópico. Vários valores ou uma configuração incompleta limitam a certeza a `inferred`. A busca de um receptor C# para ao atingir 32 passos. `[ts.rs:messages]` `[ts.rs:configured_topic]` `[csharp.rs:CHANNEL_CLIENT_DEPTH]`

## Efeitos colaterais

Os leitores geram mensagens em memória a partir dos fatos extraídos e não abrem conexões com brokers. `[message.rs:Read]` `[ts.rs:messages]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it -- channels:: channel_clients:: kafkajs_classes:: kafka_env::`.
