# Versões de workspace e owners

## Importar e aplicar uma versão

O workspace gerenciado separa as entradas dos repositórios dos módulos locais. Prepare um arquivo
com `[[repository]]`, usando os caminhos dos mirrors no job, e importe uma versão imutável:

```toml
[[repository]]
repo = "github.exemplo.internal/acme/api"
path = "/var/lib/meada/mirrors/api"
```

```sh
meada workspace import repositories.toml --message "Add API repository" --workspace /etc/meada/job.workspace.toml
meada workspace list --workspace /etc/meada/job.workspace.toml
meada workspace show <version> --workspace /etc/meada/job.workspace.toml
meada workspace apply <version> --workspace /etc/meada/job.workspace.toml
meada workspace pull --workspace /etc/meada/job.workspace.toml
```

O import valida as mesmas entradas que o editor da console; reimportar o mesmo conteúdo conserva
o id e os metadados da primeira gravação. `apply` troca o pointer do workspace sob a trava, sem
publicar uma geração. `pull` instala essa versão localmente; os comandos seguintes a leem sem
abrir conexão durante a indexação. O job tem seu próprio bootstrap, `index_dir`, mirrors e
`local_dir`; compartilha com os nós e a console somente as seções que apontam ao mesmo store e
Valkey. Execute seu refresh inteiro sob uma única trava, com o fetch fora da Meada:

```sh
meada snapshot lock --workspace /etc/meada/job.workspace.toml -- sh -ec '
  meada workspace pull --workspace /etc/meada/job.workspace.toml
  ./fetch-mirrors.sh
  meada index --workspace /etc/meada/job.workspace.toml
  meada snapshot publish --workspace /etc/meada/job.workspace.toml
'
```

## Da versão à geração

A geração publicada registra a versão do workspace que a originou. Aplicar uma versão antiga
prepara o próximo refresh; os nós continuam na geração que já guardam até um novo publish e pull.
O editor permite validar, comparar, salvar uma versão e aplicá-la separadamente.

A imagem `ghcr.io/claudiomascaro/meada:<versão>-cloud` já inclui o frontend em
`/usr/share/meada/ui`. Para uma instalação por Cargo, use `cargo install meada --features cloud`,
baixe `meada-ui-<versão>.tar.gz` da mesma release, extraia e aponte `[ui] dir` ao diretório
extraído. A versão de `meada-ui.json` precisa ser igual à do binário; um frontend de outra versão
recusa a subida. Os perfis de console de [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) e [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/)
são desligados por padrão e mantêm bootstrap, admissão e armazenamento próprios.

## Ownership por equipe

!!! warning "Planejado: S3"
    [S3, #294](https://github.com/ClaudioMascaro/meada/issues/294) define ownership e autorização por equipe. As versões e o editor acima já estão entregues em S2; as regras abaixo ainda não são comportamento disponível.

Uma entrada poderá ter um `owner`, o nome exato de um grupo OIDC. O operador global delegará a primeira entrada; o operador de equipe poderá alterar apenas entradas dos seus grupos que seu grant permita ler. O viewer consultará o que seu grant admitir. Ownership nunca ampliará o grant.

Definir, substituir **ou limpar** um owner será `owner_change`, reservado ao operador global. Salvar uma proposta gerará uma versão imutável; aplicar a versão e publicar a geração continuarão sendo ações distintas. Uma aplicação somente de owners não exigirá reindexar o código.

O diagrama distingue o caminho entregue (linhas contínuas) da autorização planejada (linhas tracejadas). [S5](named-workspaces.md) acrescentará workspaces nomeados sobre a indexação compartilhada.

<!-- meada-graph:workspaces -->
