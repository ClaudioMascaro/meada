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

Em Java com Spring, a URL base pode vir de `@Value("${chave}")`, lida do `application.properties`, do `application.yml` ou do `application.yaml` em `src/main/resources` de cada módulo, sem chave nova em `meada.toml`. Uma URL literal liga pelo host em `[serve] hosts`; um placeholder `${VAR}` (com ou sem `:padrão`) usa as mesmas chaves `base_url_env` de `[[peer]]`. Um padrão cujo host algum par serve liga por esse host.
