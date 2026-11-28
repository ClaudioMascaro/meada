# Builds JVM e configuração Spring

Leitores JVM compartilhados pelos packs Java e Kotlin e pelos tipos de contrato que usam Maven, Gradle ou Spring.

## Interface

Maven e Gradle fornecem coordenadas, dependências e metadados de build. O leitor Spring resolve propriedades e devolve o valor de configuração de uma anotação `@Value` reconhecida. `[maven.rs:read]` `[gradle.rs:read]` `[mod.rs:Build]` `[spring.rs:resolve]` `[spring.rs:inject]`

## Invariantes

As coordenadas literais validam grupo e artefato. A configuração Spring considera os arquivos base do módulo e dá prioridade a `application.properties` sobre YAML. `[mod.rs:coordinate]` `[spring.rs:Configs]`

O leitor Gradle devolve um resumo vazio quando os blocos ultrapassam 64 níveis de aninhamento. `[gradle.rs:GRADLE_DEPTH]`

## Efeitos colaterais

Os leitores interpretam o conteúdo fornecido sem executar Maven, Gradle ou a aplicação Spring. `[maven.rs:read]` `[gradle.rs:read]` `[spring.rs:Configs]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it jvmpkg::`.
