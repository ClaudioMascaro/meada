# Divergências dos contratos de diretório

Comparação dos contratos em português, inglês e espanhol com a implementação da branch empilhada sobre `a81a1767`. Foram examinados 204 READMEs em 68 diretórios. As cinco lacunas abaixo afetam quatro contratos nas três línguas. Cada observação indica uma correção documental e a fonte correspondente.

A revisão cobre as afirmações dos contratos, os links entre componentes e os símbolos citados. A leitura semântica por área foi amostral. “No drift detected.” indica que a revisão não encontrou uma diferença naquele documento.

## README.md

No drift detected.

## README.en.md

No drift detected.

## README.es.md

No drift detected.

## deploy/README.md

No drift detected.

## deploy/README.en.md

No drift detected.

## deploy/README.es.md

No drift detected.

## deploy/compose/README.md

No drift detected.

## deploy/compose/README.en.md

No drift detected.

## deploy/compose/README.es.md

No drift detected.

## examples/README.md

No drift detected.

## examples/README.en.md

No drift detected.

## examples/README.es.md

No drift detected.

## meada-core/README.md

No drift detected.

## meada-core/README.en.md

No drift detected.

## meada-core/README.es.md

No drift detected.

## meada-core/src/README.md

- O contrato lista `[[channel_client]]`. Falta especificar que o único broker aceito é `kafka` e que `topic` recebe uma posição decimal ou uma posição seguida de um único campo com nome de identificador ASCII. Incluir as duas formas aceitas junto à declaração. [meada-core/src/manifest.rs (linha 123)](./meada-core/src/manifest.rs#L123)
- A descrição de `[[helm_env]]` omite a validação da chave pontuada. Um componente vazio ou com `*` invalida o manifesto. Registrar a restrição junto à descrição do caminho Helm. [meada-core/src/manifest.rs (linha 62)](./meada-core/src/manifest.rs#L62)

## meada-core/src/README.en.md

- A tradução apresenta as mesmas lacunas descritas em [meada-core/src/README.md](#meada-coresrcreadmemd).

## meada-core/src/README.es.md

- A tradução apresenta as mesmas lacunas descritas em [meada-core/src/README.md](#meada-coresrcreadmemd).

## scripts/README.md

- A verificação indicada cobre o seletor de verificações. O teste que compara as recusas com o motor de expressões regulares do Go é ignorado por padrão. Acrescentar o comando explícito `cargo test --lib alert::regexp::tests::refusal_agrees_with_go -- --ignored`. [src/alert/regexp.rs (linha 1021)](./src/alert/regexp.rs#L1021)

## scripts/README.en.md

- A tradução apresenta as mesmas lacunas descritas em [scripts/README.md](#scriptsreadmemd).

## scripts/README.es.md

- A tradução apresenta as mesmas lacunas descritas em [scripts/README.md](#scriptsreadmemd).

## scripts/docs/README.md

No drift detected.

## scripts/docs/README.en.md

No drift detected.

## scripts/docs/README.es.md

No drift detected.

## src/README.md

No drift detected.

## src/README.en.md

No drift detected.

## src/README.es.md

No drift detected.

## src/accounts/README.md

No drift detected.

## src/accounts/README.en.md

No drift detected.

## src/accounts/README.es.md

No drift detected.

## src/alert/README.md

No drift detected.

## src/alert/README.en.md

No drift detected.

## src/alert/README.es.md

No drift detected.

## src/channel/README.md

No drift detected.

## src/channel/README.en.md

No drift detected.

## src/channel/README.es.md

No drift detected.

## src/config_values/README.md

No drift detected.

## src/config_values/README.en.md

No drift detected.

## src/config_values/README.es.md

No drift detected.

## src/contract/README.md

No drift detected.

## src/contract/README.en.md

No drift detected.

## src/contract/README.es.md

No drift detected.

## src/contract/azure/README.md

No drift detected.

## src/contract/azure/README.en.md

No drift detected.

## src/contract/azure/README.es.md

No drift detected.

## src/contract/azure/sdk/README.md

No drift detected.

## src/contract/azure/sdk/README.en.md

No drift detected.

## src/contract/azure/sdk/README.es.md

No drift detected.

## src/contract/gcp/README.md

No drift detected.

## src/contract/gcp/README.en.md

No drift detected.

## src/contract/gcp/README.es.md

No drift detected.

## src/contract/gcp/sdk/README.md

No drift detected.

## src/contract/gcp/sdk/README.en.md

No drift detected.

## src/contract/gcp/sdk/README.es.md

No drift detected.

## src/contract/sdk/README.md

No drift detected.

## src/contract/sdk/README.en.md

No drift detected.

## src/contract/sdk/README.es.md

No drift detected.

## src/decider/README.md

No drift detected.

## src/decider/README.en.md

No drift detected.

## src/decider/README.es.md

No drift detected.

## src/directory/README.md

No drift detected.

## src/directory/README.en.md

No drift detected.

## src/directory/README.es.md

No drift detected.

## src/document/README.md

No drift detected.

## src/document/README.en.md

No drift detected.

## src/document/README.es.md

No drift detected.

## src/dotnet/README.md

No drift detected.

## src/dotnet/README.en.md

No drift detected.

## src/dotnet/README.es.md

No drift detected.

## src/github_app/README.md

No drift detected.

## src/github_app/README.en.md

No drift detected.

## src/github_app/README.es.md

No drift detected.

## src/grant/README.md

No drift detected.

## src/grant/README.en.md

No drift detected.

## src/grant/README.es.md

No drift detected.

## src/http/README.md

No drift detected.

## src/http/README.en.md

No drift detected.

## src/http/README.es.md

No drift detected.

## src/http/csharp/README.md

No drift detected.

## src/http/csharp/README.en.md

No drift detected.

## src/http/csharp/README.es.md

No drift detected.

## src/http/go/README.md

No drift detected.

## src/http/go/README.en.md

No drift detected.

## src/http/go/README.es.md

No drift detected.

## src/http/java/README.md

No drift detected.

## src/http/java/README.en.md

No drift detected.

## src/http/java/README.es.md

No drift detected.

## src/http/kotlin/README.md

No drift detected.

## src/http/kotlin/README.en.md

No drift detected.

## src/http/kotlin/README.es.md

No drift detected.

## src/http/python/README.md

No drift detected.

## src/http/python/README.en.md

No drift detected.

## src/http/python/README.es.md

No drift detected.

## src/http/rust/README.md

No drift detected.

## src/http/rust/README.en.md

No drift detected.

## src/http/rust/README.es.md

No drift detected.

## src/http/swift/README.md

No drift detected.

## src/http/swift/README.en.md

No drift detected.

## src/http/swift/README.es.md

No drift detected.

## src/http/ts/README.md

No drift detected.

## src/http/ts/README.en.md

No drift detected.

## src/http/ts/README.es.md

No drift detected.

## src/http/ts/config/README.md

No drift detected.

## src/http/ts/config/README.en.md

No drift detected.

## src/http/ts/config/README.es.md

No drift detected.

## src/iac/README.md

No drift detected.

## src/iac/README.en.md

No drift detected.

## src/iac/README.es.md

No drift detected.

## src/iac/actions/README.md

No drift detected.

## src/iac/actions/README.en.md

No drift detected.

## src/iac/actions/README.es.md

No drift detected.

## src/iac/http/README.md

No drift detected.

## src/iac/http/README.en.md

No drift detected.

## src/iac/http/README.es.md

No drift detected.

## src/iac/mapping/README.md

No drift detected.

## src/iac/mapping/README.en.md

No drift detected.

## src/iac/mapping/README.es.md

No drift detected.

## src/iac/workload/README.md

No drift detected.

## src/iac/workload/README.en.md

No drift detected.

## src/iac/workload/README.es.md

No drift detected.

## src/image/README.md

No drift detected.

## src/image/README.en.md

No drift detected.

## src/image/README.es.md

No drift detected.

## src/index/README.md

No drift detected.

## src/index/README.en.md

No drift detected.

## src/index/README.es.md

No drift detected.

## src/jvm/README.md

No drift detected.

## src/jvm/README.en.md

No drift detected.

## src/jvm/README.es.md

No drift detected.

## src/kube/README.md

No drift detected.

## src/kube/README.en.md

No drift detected.

## src/kube/README.es.md

No drift detected.

## src/lang/README.md

No drift detected.

## src/lang/README.en.md

No drift detected.

## src/lang/README.es.md

No drift detected.

## src/lang/go/README.md

No drift detected.

## src/lang/go/README.en.md

No drift detected.

## src/lang/go/README.es.md

No drift detected.

## src/lang/python/README.md

No drift detected.

## src/lang/python/README.en.md

No drift detected.

## src/lang/python/README.es.md

No drift detected.

## src/lang/rust/README.md

No drift detected.

## src/lang/rust/README.en.md

No drift detected.

## src/lang/rust/README.es.md

No drift detected.

## src/lang/swift/README.md

No drift detected.

## src/lang/swift/README.en.md

No drift detected.

## src/lang/swift/README.es.md

No drift detected.

## src/lang/ts/README.md

No drift detected.

## src/lang/ts/README.en.md

No drift detected.

## src/lang/ts/README.es.md

No drift detected.

## src/names/README.md

No drift detected.

## src/names/README.en.md

No drift detected.

## src/names/README.es.md

No drift detected.

## src/names/python/README.md

No drift detected.

## src/names/python/README.en.md

No drift detected.

## src/names/python/README.es.md

No drift detected.

## src/oidc/README.md

No drift detected.

## src/oidc/README.en.md

No drift detected.

## src/oidc/README.es.md

No drift detected.

## src/package/README.md

No drift detected.

## src/package/README.en.md

No drift detected.

## src/package/README.es.md

No drift detected.

## src/package/npm/README.md

No drift detected.

## src/package/npm/README.en.md

No drift detected.

## src/package/npm/README.es.md

No drift detected.

## src/package/swiftpm/README.md

No drift detected.

## src/package/swiftpm/README.en.md

No drift detected.

## src/package/swiftpm/README.es.md

No drift detected.

## src/resolve/README.md

No drift detected.

## src/resolve/README.en.md

No drift detected.

## src/resolve/README.es.md

No drift detected.

## src/resolve/ts/README.md

- A regra sobre a consulta a escritas de produção e de teste descreve os campos. Para métodos de objetos literais, uma escrita no índice de testes também pode invalidar a resolução em produção. Documentar esse caso em `ts_literal_method`. [src/resolve/ts/fields.rs (linha 99)](./src/resolve/ts/fields.rs#L99)

## src/resolve/ts/README.en.md

- A tradução apresenta as mesmas lacunas descritas em [src/resolve/ts/README.md](#srcresolvetsreadmemd).

## src/resolve/ts/README.es.md

- A tradução apresenta as mesmas lacunas descritas em [src/resolve/ts/README.md](#srcresolvetsreadmemd).

## src/resource/README.md

No drift detected.

## src/resource/README.en.md

No drift detected.

## src/resource/README.es.md

No drift detected.

## src/snapshot/README.md

No drift detected.

## src/snapshot/README.en.md

No drift detected.

## src/snapshot/README.es.md

No drift detected.

## src/trace/README.md

No drift detected.

## src/trace/README.en.md

No drift detected.

## src/trace/README.es.md

No drift detected.

## src/ui/README.md

No drift detected.

## src/ui/README.en.md

No drift detected.

## src/ui/README.es.md

No drift detected.

## src/ui/accounts/README.md

No drift detected.

## src/ui/accounts/README.en.md

No drift detected.

## src/ui/accounts/README.es.md

No drift detected.

## src/ui/workspaces/README.md

No drift detected.

## src/ui/workspaces/README.en.md

No drift detected.

## src/ui/workspaces/README.es.md

No drift detected.

## src/workspaces/README.md

No drift detected.

## src/workspaces/README.en.md

No drift detected.

## src/workspaces/README.es.md

No drift detected.

## ui/README.md

No drift detected.

## ui/README.en.md

No drift detected.

## ui/README.es.md

No drift detected.

## ui/scripts/README.md

No drift detected.

## ui/scripts/README.en.md

No drift detected.

## ui/scripts/README.es.md

No drift detected.

## ui/src/README.md

- O contrato cita a verificação de PKCE no callback. Falta a condição da descoberta OIDC que também pode impedir o login. Se o emissor anuncia uma lista de métodos e ela omite `S256`, a descoberta falha. Registrar essa condição na descrição da autenticação. [ui/src/auth.ts (linha 50)](./ui/src/auth.ts#L50)

## ui/src/README.en.md

- A tradução apresenta as mesmas lacunas descritas em [ui/src/README.md](#uisrcreadmemd).

## ui/src/README.es.md

- A tradução apresenta as mesmas lacunas descritas em [ui/src/README.md](#uisrcreadmemd).
