# Projetos e configuração .NET

Leitores compartilhados de projetos MSBuild, referências entre projetos e configuração .NET.

## Interface

O leitor extrai propriedades, usings e referências do projeto SDK e dos `Directory.Build.props` fornecidos. A configuração resolve chaves como `A:B` no `appsettings.json` do projeto. `[msbuild.rs:read]` `[msbuild.rs:chain]` `[references.rs:referenced]` `[appsettings.rs:Settings]`

## Invariantes

A busca por configuração compara segmentos sem distinguir maiúsculas e minúsculas e aceita apenas valores de texto. A travessia de referências limita a profundidade. `[appsettings.rs:get]` `[references.rs:REFERENCE_DEPTH]`

A cadeia de importações de `Directory.Build.props` inclui até oito arquivos. `[msbuild.rs:chain]` `[msbuild.rs:PROPS_DEPTH]`

## Efeitos colaterais

Os leitores usam os bytes e projetos oferecidos pelo chamador sem iniciar MSBuild ou o SDK do .NET. `[msbuild.rs:read]` `[references.rs:referenced]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it nugetpkg::`.
