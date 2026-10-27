# Configuração

Cada repositório pode ter um `meada.toml` na raiz, versionado no git, dizendo com quem ele
conversa. Quem consome uma rota nomeia o par:

```toml
[[peer]]
repo = "example.com/acme/api"            # remoto git de origin, sem esquema nem .git
base_url_env = ["API_URL"]               # variável de onde o cliente lê a URL base
# path = "../api"                        # opcional: dica de onde o clone está
```

Quem serve a rota declara os hosts em que responde:

```toml
[serve]
hosts = ["api.example.com"]
prefix = "/shop"                         # opcional: o gateway põe isto na frente dos caminhos
strip = "/v1"                            # opcional: e tira isto do começo deles
```

Com `prefix` e `strip`, uma chamada a `/shop/orders` chega à rota `/v1/orders` do código, no
nível `inferred` e com o motivo `gateway prefix /shop, gateway strip /v1`. Um segmento de versão
só sai quando você o escreve em `strip`, e quem chama o caminho do código continua ligando como
antes.

Um recurso compartilhado que nenhum código nomeia, como uma tabela ou um bucket, entra como
`[[resource]]`:

```toml
[[resource]]
kind = "postgres"                        # identificador em minúsculas
id = "orders"
role = "write"                           # read, write ou read-write
via = ["src/orders.ts::cancelOrder"]     # opcional: os símbolos que fazem o acesso
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`.
Sem `via`, ou se nenhum `via` corresponde a um símbolo, o lado é o repositório inteiro.

Meada acha os pares pelo remoto git, mesmo que o clone fique em outro caminho em cada máquina.
Uma chave desconhecida ou mal escrita dá um erro com o arquivo, a linha e a chave. Sem
`meada.toml`, o repositório não declara nenhum par.

## Artefatos JVM

Os contratos de pacotes JVM são descobertos nos manifests: `pom.xml`, `settings.gradle`, `settings.gradle.kts`, `build.gradle` e `build.gradle.kts`. Não exigem chave adicional em `meada.toml`. Declare os outros repositórios em `[[peer]]` como nos exemplos acima. Use coordenadas literais para ligações `exact`; propriedades Maven e expressões Gradle permanecem sem avaliação, e as que formam o grupo ou o artefato produzem candidatos `ambiguous`.

Em Java e Kotlin com Spring, a URL base pode vir de `@Value("${chave}")` (em Kotlin, `@Value("\${chave}")`, com o `$` escapado), lida do `application.properties`, do `application.yml` ou do `application.yaml` em `src/main/resources` de cada módulo, sem chave nova em `meada.toml`. Uma URL literal liga pelo host em `[serve] hosts`; um placeholder `${VAR}` (com ou sem `:padrão`) usa as mesmas chaves `base_url_env` de `[[peer]]`. Um padrão cujo host algum par serve liga por esse host.

Em C#, a URL base pode vir de `Environment.GetEnvironmentVariable("X")` ou de `configuration["Secao:Chave"]`, lida do `appsettings.json` do projeto (só o arquivo base), sem chave nova no `meada.toml`. Uma chave é a variável `Secao__Chave` nos `base_url_env` do `[[peer]]`; uma URL literal que o arquivo guarda para ela liga por host em `[serve] hosts` quando algum peer serve esse host.

## Pacotes NuGet

Os contratos de pacotes NuGet são descobertos nos `.csproj` com SDK. Não precisam de chave extra no `meada.toml`: um `PackageReference` liga ao provedor pelo id, e um `ProjectReference` pelo caminho do arquivo do projeto, que alcança um projeto de outro repositório pelas raízes dos repositórios do workspace (declare-o em `[[peer]]`). Valores de propriedade como `$(Version)` ficam sem avaliar.

## Pacotes Swift

Os contratos de pacote Swift são descobertos no `Package.swift`. Não precisam de chave nova no `meada.toml`: um `.package(url:)` liga ao repositório do workspace de mesma identidade de remoto e um `.package(path:)` ao repositório nesse caminho, que precisa ser um `[[peer]]` do workspace quando é outro repositório. Use URLs, caminhos e nomes literais para ligações `exact`.

