# Imagens de contêiner e código executado

Liga quem constrói uma imagem aos workloads que a executam e ao código local incluído na construção.

## Interface

O leitor resume configurações de build, incluindo destinos do Jib e entradas do Dockerfile. A preparação resolve nomes de imagem com padrões e a projeção de ambiente mantém o contêiner que executa o código. `[config.rs:read]` `[jib.rs:gradle]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]` `[environment.rs:scoped_values]`

Com `[[helm_env]]`, o leitor extrai as entradas escalares da chave declarada em charts com `Chart.yaml`. Cada `values-<env>.yaml` ou `.yml` sobrescreve os valores da base. Na ausência dessas variantes, a base forma um único ambiente. `[config.rs:chart_envs]` `[config.rs:laid]`

## Invariantes

Quando não consegue determinar as entradas do Dockerfile, o leitor pode atribuir o contexto inteiro com evidência inferida. Caminhos de código verificados permanecem dentro do repositório. `[dockerfile.rs:executions]` `[code.rs:checked]`

Apenas o pacote dono do chart recebe esse ambiente para resolver canais. Se todos os ambientes dos charts que definem a variável concordarem no literal, a evidência pode atingir `declared`. Ausência em um desses ambientes ou valores divergentes limitam os resultados a `inferred`. `[environment.rs:entries]` `[environment.rs:values]` `[environment.rs:resolve]`

## Efeitos colaterais

A indexação verifica caminhos no checkout e lê o Dockerfile. A preparação dos candidatos a imagem atualiza as exportações em memória. `[code.rs:checked]` `[dockerfile.rs:executions]` `[patterns.rs:prepare]`

O leitor consulta a lista de arquivos do chart e a base no checkout. Ao exceder o orçamento do arquivo, omite o próximo builder ou descarta o ambiente Helm inteiro. `[config.rs:chart_envs]` `[config.rs:builder]` `[config.rs:IMAGE_STORED_MAX]`

## Verificação

Na raiz do repositório, execute `cargo test -p meada --test it -- images:: helm_env::`.
