# Snapshots

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Um job indexa e publica; os nós recebem a geração pronta. Explore a sequência abaixo e configure o store para publicar e receber os bundles.


<!-- meada-graph:snapshot-lifecycle -->

<details markdown="1" class="reference-details">
<summary>Validação antes da publicação e formato da geração</summary>

**Snapshot** (feature `snapshot`, `cargo install meada --features snapshot`). Um job indexa e
publica; os nós recebem o resultado inteiro, em vez de cada um indexar os próprios espelhos.
Depois de `meada index --workspace`, que continua sem rede, o job roda
`meada snapshot publish --workspace <manifesto>`. Ele confere, só lendo, que cada índice é igual
ao seu espelho e que cada espelho está limpo (`git status` sem nada, nem arquivo ignorado), e
recusa listando cada repositório e o comando `meada index --workspace` que resolve. Então envia
uma geração: um bundle por repositório (um tar do espelho, `.git` incluído, e do seu
`index.db`), um documento que os lista com tamanho e hash BLAKE3, e por último o ponteiro
`current`. A única linha em stdout é o id da geração. Um bundle ou uma geração já guardados não
são enviados de novo, e nada é escrito na máquina que publica.

</details>

## Configurar o store

```toml
[snapshot]
endpoint = "https://s3.us-east-1.amazonaws.com"   # qualquer endpoint compatível com S3
region = "us-east-1"
bucket = "exemplo-meada"
prefix = "prod"                                    # opcional
path_style = false                                 # true na maioria dos stores self-hosted
# ca_file = "ca.pem"                              # CA privada opcional, relativa ao manifesto
credentials = "static"                             # ou "web-identity", "pod-identity", "instance"
access_key_id_env = "AWS_ACCESS_KEY_ID"            # padrões; ou as chaves *_file
secret_access_key_env = "AWS_SECRET_ACCESS_KEY"
session_token_env = "AWS_SESSION_TOKEN"            # opcional
local_dir = "/var/lib/meada/generations"           # para o pull e o serviço das gerações
```

## Credenciais e transporte

<details markdown="1" class="reference-details">
<summary>Escolher a fonte de credenciais</summary>

O publish só conecta ao `endpoint`, com TLS fora de loopback, e ao endpoint de credencial da
fonte escolhida; nenhuma requisição segue um redirecionamento nem passa por proxy. As
credenciais vêm de uma fonte só, a que `credentials` nomeia: `static` lê a chave e o segredo das
variáveis ou dos arquivos que a seção nomeia (nunca do manifesto); `web-identity` (IRSA) lê
`AWS_WEB_IDENTITY_TOKEN_FILE` e `AWS_ROLE_ARN` e troca o token no STS da `region`, por https;
`pod-identity` (EKS Pod Identity) chama o agente em `AWS_CONTAINER_CREDENTIALS_FULL_URI`, só
`http://169.254.170.23` ou `http://[fd00:ec2::23]`, com o token de
`AWS_CONTAINER_AUTHORIZATION_TOKEN_FILE`; `instance` usa o serviço de metadados da instância
(IMDSv2, só por IPv4, `http://169.254.169.254`) e só quando escrito na seção. As credenciais ficam só em
memória, e nenhum segredo aparece num erro.

</details>

`ca_file` acrescenta certificados PEM às raízes de confiança do sistema. O arquivo tem no
máximo 1 MiB; certificados inválidos ou um arquivo vazio são recusados, nomeando o arquivo.

## Receber uma geração

Em cada nó, `meada snapshot pull --workspace <manifesto>` lê o ponteiro `current` do store e,
quando ele nomeia uma geração que o nó ainda não tem, baixa os bundles para `local_dir`, confere
o tamanho e o hash BLAKE3 de cada um contra o documento, extrai e troca o ponteiro do nó por
rename. A única linha em stdout é o id da geração atual. `--every <segundos>` repete o pull (e só
imprime quando o ponteiro muda); um segundo pull no mesmo `local_dir` sai com 0 avisando que já
há um em andamento. O nó guarda a geração atual e a anterior e apaga as mais antigas.

## Validar os bundles

<details markdown="1" class="reference-details">
<summary>Regras de validação e isolamento dos bundles</summary>

Tudo o que vem do store é tratado como entrada hostil: ids e identidades são conferidos antes de
virar caminho; o pull recusa no arquivo um caminho absoluto, um `..`, um link que sai do
repositório, uma entrada gravada através de um link simbólico e dispositivos ou FIFOs, com tetos
de bytes e de entradas por geração. O `.git/config` extraído é trocado por um mínimo que o Meada
escreve (só `[core]` e o `origin` da identidade), `.git/hooks` é apagado, e o `git` do Meada
roda nesses espelhos sem a configuração do sistema nem a global. Um pull que falha em qualquer
passo deixa o ponteiro e as gerações guardadas como estavam. Ao servir, e quando um manifesto
do modo servidor lista um espelho, o `meada.toml` e o `meada.decisions.toml` dele são lidos só
como arquivos regulares de até 1 MiB: um link simbólico é recusado sem ser aberto, mesmo
apontando para dentro do espelho, e a geração ou a entrada não é servível, com o erro nomeando o
arquivo. Um checkout local, que é do desenvolvedor, continua seguindo links.

</details>

```
<local_dir>/
  current                     # "<id da geração> <fence>", fence 0 sem a trava de refresh
  pull.lock
  <id da geração>/
    generation.json           # o documento, como guardado no store
    <id do bundle>/index.db
    <id do bundle>/mirror/    # o checkout, .git incluído
```

## Servir sem reindexar

Para servir o que o nó recebeu, `meada mcp --listen <endereço> --workspace <manifesto>
--generations`. O manifesto do nó contribui só com as seções de módulo (`[snapshot]`); os
repositórios são os que a geração atual registrou, com os espelhos e índices dentro de
`<local_dir>/<id da geração>/`. Cada chamada lê `<local_dir>/current` e abre o motor sobre a
geração que ele nomeia: uma chamada já aberta termina na geração em que começou, a seguinte vê a
nova, e um `meada snapshot pull --every` troca a geração sem reiniciar o servidor (um
repositório novo numa geração nova ganha o seu endpoint na hora). Nada é escrito por chamada,
os índices abrem só para leitura e o servidor não abre conexão alguma com o store. Os nós nunca
rodam `meada index`: quem indexa é o job, e o nó só recebe. Sem geração atual, ou com um índice
que não está na versão de schema deste binário, o servidor recusa subir e nomeia
`meada snapshot pull` ou o repositório e a geração.

## Prontidão

`meada status --workspace <manifesto> --generations` é a sonda de prontidão do nó. Imprime o id
da geração atual, os ids guardados e uma linha `store: reachable (current <id>)` ou
`store: unreachable (<motivo>)`. O código de saída depende só do estado local: 1 sem uma
geração atual servível (os problemas vão para stderr), 0 do contrário, com o store alcançável
ou não, de modo que um store fora do ar não tira o nó da rotação. `meada export --workspace
<manifesto> --repo <identidade> --generations` exporta do mesmo jeito.
