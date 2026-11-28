# Adaptadores de linguagem

[Português](./README.md) · [English](./README.en.md) · [Español](./README.es.md)

## Visão geral

Cada adaptador transforma código-fonte em fatos que a resolução e os extratores de contratos
compartilham.

## Interface

`LanguagePack` define análise por arquivo, configuração, junção de fatos e resolução de módulos.
`pack_for` escolhe pelo caminho. O registro reúne JavaScript/TypeScript, Python, Rust, Go, Java,
Kotlin, C# e Swift.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:pack_for](./mod.rs) [mod.rs:PACKS](./mod.rs)

O adaptador C# reúne os `global using` do projeto antes da resolução. Java e Kotlin leem o layout
JVM. Outros adaptadores mantêm sua própria lógica de descoberta de módulos.
[csharp.rs:join](./csharp.rs) [java.rs:layout](./java.rs) [kotlin.rs:resolves_with](./kotlin.rs)

Go registra usos de tipos nomeados nas assinaturas e declarações. Tipos predefinidos e parâmetros
de tipo ficam de fora. O resolvedor descarta referências Go sem alvo no repositório.
[go.rs:type_uses](./go.rs) [go.rs:PREDECLARED](./go.rs) [go.rs:drops_external_uses](./go.rs) [../resolve.rs:reference](../resolve.rs)

`Facts` guarda por arquivo os receptores e suas classes (`TsLocal`, `TsClass`), os campos Go
associados ao ambiente (`EnvField`) e as atribuições observadas em campos privados
(`FieldWrites`). A resolução entre arquivos e a leitura dos contratos usam esses fatos.
[mod.rs:Facts](./mod.rs) [mod.rs:TsLocal](./mod.rs) [mod.rs:TsClass](./mod.rs) [mod.rs:EnvField](./mod.rs) [mod.rs:FieldWrites](./mod.rs)

- [Seleção de arquivos e valores Go](./go/README.md)
- [Metadados de projetos Python](./python/README.md)
- [Módulos, macros e valores Rust](./rust/README.md)
- [Módulos e valores Swift](./swift/README.md)
- [Referências, receptores e usos em TypeScript e JavaScript](./ts/README.md)

## Invariantes

Um pacote recebe os bytes e o caminho relativo do arquivo. As configurações e os fatos dos demais
arquivos vêm do indexador. Kotlin compartilha com Java o grupo de resolução.
[mod.rs:LanguagePack](./mod.rs) [mod.rs:resolver_of](./mod.rs) [kotlin.rs:resolves_with](./kotlin.rs)

O pack C# acompanha até 128 tipos envolventes ao extrair definições aninhadas. Java e Kotlin
marcam métodos privados para o resolvedor reconhecer chamadas que só podem executar aquele corpo.
[csharp.rs:DEFINITION_DEPTH](./csharp.rs) [java.rs:parse](./java.rs) [kotlin.rs:parse](./kotlin.rs) [../resolve.rs:runs_alone](../resolve.rs)

## Efeitos colaterais

O resultado da análise é `Facts`. `hash_spans` preenche hashes a partir dos bytes recebidos.
`with_values` habilita temporariamente a captura de valores na thread e restaura o estado ao sair.
[mod.rs:Facts](./mod.rs) [mod.rs:hash_spans](./mod.rs) [mod.rs:with_values](./mod.rs)

## Verificação

```sh
bounded cargo test --locked -p meada --lib --test it -- lang:: xlang:: uses:: ts_receivers:: ts_library_receivers:: field_flow:: http_bases::
```
