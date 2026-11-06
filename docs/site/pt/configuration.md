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
via = ["src/orders.ts::cancelOrder"]     # opcional: os locais que fazem o acesso
```

Cada escritor liga a cada leitor do mesmo recurso em outro repositório, no nível `declared`.
Sem `via`, ou se nenhum `via` corresponde a um símbolo, nó IaC ou chave, o lado é o repositório inteiro.

Os SDKs AWS de Java 2.x, Go v2 e .NET também nomeiam recursos diretamente pelo código: por exemplo, `PutObject` em `uploads` escreve `resource::s3::uploads`, e `GetObject` no mesmo bucket lê esse contrato. A ligação entre usos escritos é `exact`; uma declaração explícita continua `declared`. O bloco Terraform `aws_s3_bucket` do mesmo nome liga a todos esses usos como provisionador. Não é preciso repetir os usos como `[[resource]]`. Construa o cliente no código ou passe-o por chamadas exatas; o tipo de uma interface injetada sozinho não basta. Nomes lidos do ambiente ficam sem resolução, com a variável no export. Consulte as [operações reconhecidas](capabilities.md#sdks-aws-em-java-go-e-c).

Uma rota que só um arquivo de teste registra (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`,
`*_test.py`, um diretório `tests` ou `__tests__`, `src/test` em JVM) não é provedora do
repositório: um roteador ou um upstream falso de teste não serve nada, e deixá-lo contar faria
o repositório cliente parecer provedor da rota que ele consome. O código de teste continua
indexado, com chamadas e símbolos. Quem quer essas rotas como provedoras liga a chave na
raiz do `meada.toml`:

```toml
test_routes = true                       # opcional: rotas registradas em arquivos de teste servem
```

A chave vale só no `meada.toml` do repositório (uma entrada de manifesto de workspace não a tem) e só para rotas HTTP; a rota conta pelo arquivo onde o handler é definido.

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


## O que é indexado

A varredura cobre todos os arquivos do repositório, versionados ou não, exceto o que estes excluem:

- **Sempre ignorados**, com ou sem arquivo de ignore que os liste: diretórios chamados `node_modules` ou `Pods`, um diretório `Checkouts` dentro de `Carthage`, arquivos chamados `*.min.js` ou `*.min.css` e a saída de build do .NET (`obj` ou `bin` ao lado de um `.csproj`). São caches de dependências e artefatos de build, nunca código próprio.
- **`.gitignore` e `.ignore`**, respeitados com ou sem git, na raiz e em subdiretórios, e só dentro do repositório.
- **`.meadaignore`**, com a sintaxe do `.gitignore`, lido da mesma forma. Ele não traz de volta um arquivo que os padrões ignoram.

`vendor`, `dist`, `build` e código gerado (as migrações do Entity Framework, por exemplo) não são ignorados por padrão: qualquer um pode ser código próprio ou conter um contrato, e o Meada não sabe distinguir. Liste no `.meadaignore` o que você não quer indexar:

```gitignore
vendor/
dist/
build/
Migrations/
*ModelSnapshot.cs
*.Designer.cs
```

Um arquivo que um novo padrão ou uma nova entrada exclui sai do índice no próximo refresh, como um arquivo apagado.

## Locais de recursos declarados

O `via` de `[[resource]]` aceita símbolos (`src/orders.ts::cancelOrder`), endereços IaC e
`<arquivo>#<caminho da chave>`. O lado fica no bloco ou na chave, alcançável por `context` e
`trace`, e suas ligações continuam `declared`. Os endereços incluem Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) e Serverless
(`serverless.yml::functions.api`). Use `<arquivo>::<endereço>` para distinguir arquivos. O local identifica o bloco de origem;
o qualificador ` (root ...)` das consultas IaC não é uma forma de `via`.
Um endereço que corresponde a vários blocos de origem precisa do qualificador de arquivo;
sem ele, o `via` é obsoleto.

Caminhos de chave usam pontos, índices (`groups[0].name`) e chaves entre aspas JSON
(`metadata.labels["app.kubernetes.io/name"]`). Em listas, `[chave=valor]` seleciona os itens
pela igualdade do valor completo, por exemplo
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
A localização vem de valores indexados e seus caminhos pais.
Valores nulos e contêineres vazios não têm valor indexado; seu `via` é obsoleto.
Um seletor pode encontrar vários itens; cada chave encontrada recebe um lado. Valores
marcados como `redacted` ou cortados não servem para selecionar um item. Expressões e templates
HCL não satisfazem a igualdade de um seletor. Os arquivos seguem as regras do
índice, incluindo `.meadaignore`; nenhum arquivo é aberto por causa de um `via`.

Um `via` sem correspondência é obsoleto e aparece nas linhas existentes de `meada status`.
Sem nenhum `via` válido, o lado volta ao repositório inteiro. Isso permite declarar o vínculo
entre rótulos em valores Helm e regras de roteamento sem renderizar templates.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```
