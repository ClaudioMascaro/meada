# Workspaces nomeados

!!! warning "Parcialmente entregue: S5, contratos decididos; o catálogo, o primeiro workspace, suas versões, a configuração por contexto, a autoria guiada e as propostas MCP já existem"
    [S5, #339](https://github.com/ClaudioMascaro/meada/issues/339) é escopo sequencial aceito após S3 e S4. A [consolidação dos contratos, #340](https://github.com/ClaudioMascaro/meada/issues/340), está concluída: os ADRs 0020, 0021 e 0022 fixam os contratos descritos aqui. Do runtime, duas fatias estão entregues: o **catálogo** ([#341](https://github.com/ClaudioMascaro/meada/issues/341)), descrito em [o catálogo](github-app.md#o-catalogo-de-repositorios) e no [console](console.md#catalogo-de-repositorios), e o **primeiro workspace** ([#342](https://github.com/ClaudioMascaro/meada/issues/342)): `[workspaces]`, criação, listagem e consulta, ingestão compartilhada com a regra do patrocinador e a rota do nó, descritos no [console](console.md#workspaces-nomeados). O **ciclo de vida das versões** ([#343](https://github.com/ClaudioMascaro/meada/issues/343)) também está entregue: salvar, aplicar, reverter, renomear, apagar e delegar, com a leitura de versões e histórico, descrito no [console](console.md#workspaces-nomeados) e abaixo. A **configuração de contexto** ([#344](https://github.com/ClaudioMascaro/meada/issues/344)) também está entregue: peers, referências a variáveis de ambiente, recursos e configurações de serve por workspace, com a visão da configuração efetiva e um editor de texto no console, descritas em [configuração efetiva](#configuracao-efetiva). A **autoria guiada** ([#345](https://github.com/ClaudioMascaro/meada/issues/345)) também está entregue: controles de formulário sobre o mesmo manifesto, descritos em [autoria guiada](#autoria-guiada). As **propostas MCP** ([#346](https://github.com/ClaudioMascaro/meada/issues/346)) também estão entregues, descritas em [autoria assistida por MCP](#autoria-assistida-por-mcp). O restante abaixo não é uma opção de configuração por enquanto. Sem `[workspaces]` o console continua operando com um workspace por deployment.

## Compor sem duplicar a indexação

Workspaces nomeados se sobrepõem sobre a indexação compartilhada. Um repositório não ganha outro índice, job, trava de refresh ou geração por pertencer a dois workspaces: um único refresh busca, indexa e publica cada repositório selecionado uma vez, e cada workspace é um contexto que seleciona alguns deles. Há três papéis humanos: administrador da plataforma, operador de workspace e viewer.

O grant efetivo por repositório continua sendo a autoridade. Com Y legível pelos grupos A e B, o repositório A só pelo grupo A e o repositório B só pelo grupo B, um operador apenas em A pode compor Y+A e tem Y+B recusado; um operador apenas em B pode compor Y+B e tem Y+A recusado; quem está nos dois pode compor os três. Compartilhar Y não concede A nem B a ninguém, e um link de Y para B não aparece no workspace de Y+A.

<!-- meada-graph:workspace-overlap -->

## O primeiro workspace

Um mantenedor admitido, sem nenhuma entrada própria, abre **Workspaces nomeados** no console, escolhe repositórios do catálogo, dá um nome ao workspace e confirma o manifesto que a página mostra. O servidor decide de novo em uma transação contra o conjunto de permissões mais novo: todo repositório selecionado precisa estar no catálogo e o login do GitHub da pessoa precisa ter um papel aceito nele, direto ou por um grupo. Uma recusa nomeia os repositórios que a concessão da pessoa lê e apenas conta os demais. O workspace é gravado na identidade da pessoa, que o opera; colegas cujas concessões próprias leem os mesmos repositórios o veem sem convite algum e não podem alterá-lo.

O workspace passa a poder ser consultado pelo job compartilhado: `meada workspace pull` o lê, `meada github-app roles` deixa de fora, antes da busca, toda entrada em que nenhum de seus operadores tem um papel aceito (uma decisão de política, não uma busca que falhou) e os repositórios que só workspaces nomeados selecionam são buscados, indexados e publicados uma vez, em `<[workspaces] mirrors>/<identity>`. Até uma geração registrar o workspace, o console o mostra como `applied` com as entradas `pending`; depois de registrado ele é `live` e as entradas `ready`. Uma busca ou indexação que falha continua abortando toda a publicação e mantendo a geração atual.

## Manter um workspace

Um operador edita seu workspace como uma sequência de versões. **Salvar** guarda uma versão e não move nada; **aplicar** torna uma versão guardada a atual, e aplicar uma que já foi a atual é um **rollback**. Toda mudança leva a **base** que a página leu (o número que `GET /api/v1/workspaces/<id>` mostra): se alguém mudou o workspace desde então, a mudança é recusada (`stale_base`, informando a base atual) e nada é gravado, de modo que duas pessoas editando ao mesmo tempo nunca se sobrescrevem. No momento da gravação o servidor confere de novo, em uma transação, que o workspace ainda existe, que a base é a atual, que a pessoa ainda o opera (ou é o administrador da plataforma) e que todo repositório que a mudança **adiciona ou altera** ainda é elegível para ela no conjunto de permissões mais novo; os repositórios que uma versão mantém sem alteração e as remoções não são reconferidos. Um papel revogado, uma entrada recusada, uma página velha ou uma versão de outro workspace não mudam nada.

Um operador que perdeu o papel em um repositório mantém o workspace, lê as entradas que sua concessão lê e uma contagem das demais, e ainda pode editá-las: a versão que salva mantém no lugar, na ordem, as entradas que ele não lê, e ele não consegue lê-las nem sobrescrevê-las. Ele remove uma voltando a uma versão que não a tem, pedindo ao administrador da plataforma (cuja visão de gestão não é filtrada) ou apagando o workspace. Os colegas que perderam o papel em qualquer um dos repositórios deixam de ver o workspace, suas versões e seu histórico: recebem o 404 de um id que não existe. Um visualizador, isto é, quem lê o workspace sem operá-lo, não escreve: recebe um 403 `not_operator`; e ler um repositório nunca edita o workspace de outra pessoa.

**Renomear** muda só o nome e a descrição (o id continua). **Apagar** tira o workspace das listas e do próximo refresh, mantendo suas versões e seu histórico no Postgres; os repositórios que ele selecionava continuam indexados uma única vez para cada outro workspace que os selecione, e as gerações retidas mantêm o contexto até serem descartadas. Só o administrador da plataforma **delega** operadores (um workspace mantém ao menos um). Workspaces sobrepostos sobre o mesmo repositório são independentes: editar, reverter ou apagar um não move nada no outro. Cada operação, recusas incluídas, é um registro de auditoria com a pessoa, o workspace e a base; `meada workspaces list|show|versions|diff|create|save|apply|rename|delete|operators` fazem o mesmo pela linha de comando, como administrador da plataforma.

## Quem pode o quê

- **Administrador da plataforma**: o operador global que já existe. Configura o deployment, delega operadores e administra qualquer workspace. Nenhum papel serializado muda de nome.
- **Operador de workspace**: quem criou um workspace ou recebeu a delegação dele. Edita, aplica, reverte, renomeia e remove esse workspace e nenhum outro. Estar no mesmo grupo do criador não concede nada disso.
- **Viewer**: ninguém o atribui. Uma pessoa vê um workspace quando o próprio grant atual lê todos os repositórios dele, então colegas de grupo não precisam de convite, de grupo no provedor de identidade nem de pacote de acesso.

Criar um workspace exige um papel aceito do GitHub, direto ou por grupo do repositório, em todos os repositórios selecionados. O grant por claims continua valendo para leitura, não para criação. A elegibilidade nunca é armazenada: cada decisão lê o conjunto de permissões mais recente, e um conjunto mais antigo que `max_age_seconds` torna nada selecionável até o job gravar outro.

## O que um leitor parcial vê

Um workspace fica oculto para quem tem grant que não lê todos os repositórios dele: some das listas e responde o mesmo 404 de um id que não existe. Nomes, descrições, versões e diagnósticos seguem a mesma regra, porque um título pode revelar um repositório que a pessoa não pode ler. Operadores e o administrador da plataforma são exceção, para poderem consertar o próprio workspace: um operador que perdeu o papel em A vê os repositórios que consegue ler e a contagem dos ocultos, e toda resposta do grafo continua filtrada pelo grant. Workspaces são identificados por um id determinístico e os nomes podem se repetir, então criar um não revela nada sobre os ocultos.

A perda de um papel ou da participação em um grupo remove o acesso no próximo conjunto de permissões e nunca é revivida por um conjunto mais antigo. O workspace, suas versões e seu histórico não são apagados.

## Contratos distintos

1. **Catálogo** (entregue): os repositórios que o GitHub App cobre e que a lista de permitidos do administrador admite, listados pelo job no mesmo conjunto de permissões dos papéis. Quem consulta vê apenas os repositórios que pode selecionar; cobertura não é direito.
2. **Entradas selecionadas**: um manifesto versionado de repositórios com peers, relações de provedor, referências a variáveis de ambiente, recursos e configuração de serve. Não leva caminho de mirror, valor secreto, owner nem lista de documentos.
3. **Ingestão compartilhada**: uma entrada por repositório entre todos os workspaces. Uma entrada em que nenhum operador do seu workspace tem um papel aceito é excluída antes do fetch, o que não é um fetch com falha. Uma falha de fetch ou de índice continua abortando a publicação inteira, e todo workspace mantém a última geração completa.
4. **Versões**: salvar, aplicar, reverter, renomear e remover, cada uma conferida em uma transação contra a versão base, a autoridade de quem a faz e a elegibilidade atual, e cada uma auditada.
5. **Seleção da consulta**: `/-/workspaces/<id>/<identity>` responde a partir do workspace como uma geração o registrou, limitado pelo manifesto e pelo grant. Uma geração fixada o reproduz exatamente. A rota simples `/<identity>` continua servindo o workspace do deployment.

## Configuração efetiva

O `meada.toml` do próprio repositório continua vencendo por inteiro sobre as declarações de qualquer workspace, e o editor mostra quando uma declaração é ignorada. As declarações para repositórios sem ele são por workspace: dois workspaces que compartilham Y podem declarar peers ou hosts de serve diferentes sem um segundo índice, porque são aplicadas ao responder e nunca entram no bundle. Quais documentos são indexados pertence à ingestão e é igual em todos os workspaces.

Cada entrada do manifesto de um workspace pode trazer, além de `repo`, blocos `[repository.serve]` (`hosts`, `prefix`, `strip`), `[[repository.resource]]` e `[[repository.peer]]` com as chaves do `meada.toml`. O `base_url_env` de um peer lista **nomes** de variáveis de ambiente, nunca um valor, uma URL ou `NOME=valor`; o Meada não as lê. `path`, `owner`, `added_by` e `[repository.serve] documents` são recusados (422), porque caminhos de mirror e credenciais são da plataforma e os documentos decidem o que é indexado. As camadas nunca se misturam: o `meada.toml` do próprio repositório vale por inteiro; senão valem os blocos do workspace; senão o repositório não declara nada.

**Antes de salvar**, o editor do console (e `POST /api/v1/workspaces/effective`, que não grava nada) mostra a configuração efetiva do texto; **depois**, `GET /api/v1/workspaces/<id>/effective[?version=]` a mostra para uma versão gravada. Por entrada ela diz a `source` (`repository`, `context` ou `none`), os blocos `ignored`, a `coverage` (`indexed`, `pending_ingestion` ou `excluded`) e diagnósticos: `ignored_by_repository`, `unknown_until_ingested` (o repositório não está em nenhuma geração mantida, então ainda não se sabe se o arquivo dele substitui), `excluded_no_sponsor`, `peer_missing` (um peer fora do workspace é apontado, nunca descoberto) e `documents_not_indexed` (hosts de serve para um repositório cuja ingestão não declara `[serve]`). Um salvamento, uma aplicação e uma criação trazem os mesmos diagnósticos ao lado do resultado, então uma declaração ignorada nunca aparece como simplesmente aplicada. A visão é calculada pela mesma função que o navegador executa (WebAssembly) e nunca entra no texto do manifesto nem em id algum.

Uma mudança de configuração é uma nova geração dos mesmos bundles: nada é reindexado, uma geração fixada antes da mudança mantém a configuração que registrou, e outro workspace que compartilha o repositório não se move. A `base` do workspace é verificada como em qualquer mudança.

## Autoria guiada

A página de workspaces do console edita um workspace que você opera por **controles de formulário** e pelo **texto do manifesto**, e os dois são um só documento: uma edição em qualquer um reescreve o outro, ambos passam pelo mesmo WebAssembly do servidor (`meada-core`), e o que se grava é o texto canônico que ele escreve. O formulário não tem regra própria: o que uma entrada pode conter, quais repositórios você pode selecionar, o diff e as entradas que uma mudança precisa achar elegíveis de novo são do core, e o servidor decide outra vez na transação. Se o texto tem erros, o formulário fica somente leitura até que sejam corrigidos.

- **Repositórios** são adicionados a partir do catálogo (busca por prefixo, página a página) e removidos do manifesto. Um repositório que o catálogo não listou deve ser verificado antes (`POST /api/v1/catalog/check`, feito só quando você aperta *Verificar elegibilidade*; as outras requisições do formulário são as páginas do catálogo e *Mostrar a configuração efetiva*, e o resto é calculado localmente), e cada entrada alterada diz em texto se está *já no workspace* (verificada de novo só se você a alterar), *selecionável*, *indisponível*, *com verificação pendente* ou *ainda não verificada*; uma mudança que toca um repositório para o qual você não é elegível não pode ser salva e nomeia os que o seu grant lê e conta os demais.
- **Configurações de serve** (`hosts`, `prefix`, `strip`), **recursos** (`kind`, `id`, `role`, `via`) e **peers** (o repositório do peer e os **nomes** das variáveis de ambiente com a URL base, nunca um valor) são campos de cada repositório. Um campo esvaziado é um campo ausente, e um valor que o core recusa aparece como a mensagem do core.
- **Escopo e configuração efetiva** aparecem em texto, nunca só por cor: se você opera o workspace, quantas entradas edita e quantas não pode ler (mantidas como estão quando você salva), a ingestão de cada entrada (`pending`, `ready`, `excluded` ou nova) e, depois de *Mostrar a configuração efetiva*, qual origem vale e o que o `meada.toml` do próprio repositório ignora.
- **Revisar e salvar.** A página lista o diff (entradas adicionadas, removidas e alteradas, com as partes alteradas) e *Salvar e aplicar* abre uma confirmação que **congela** o conteúdo revisado: o texto do manifesto, a mensagem e a base. Nada além disso é enviado quando você confirma, aconteça o que acontecer com o rascunho. Uma recusa aparece como o servidor a redige: `entries_refused` nomeia os repositórios (e conta os ocultos) e lê de novo os seus papéis para que a elegibilidade mostrada seja a atual, `stale_base` oferece recarregar o workspace (uma versão gravada cuja aplicação foi recusada é dita como tal), e perder o papel de operador retira o editor. Quem só vê o workspace vê as entradas e o estado delas, mas nenhum controle que altere algo.

Calcular estas regras locais não abre conexão. Os dois idiomas do console (inglês e português do Brasil) têm os mesmos controles. Ainda não construído: uma página das versões de um workspace com reversão (as rotas `versions` e `history` existem); a interface de propostas é a [autoria assistida por MCP](#autoria-assistida-por-mcp), abaixo.

<!-- meada-graph:mcp-proposal -->

## Autoria assistida por MCP

Um **servidor de propostas** separado (feature `proposals`, [ADR 0022](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0022-authenticated-workspace-proposals.md)) permite que um agente de CLI, agindo por um operador, liste os repositórios que esse operador pode selecionar e pergunte se um manifesto é válido e autorizado. Ele deriva a identidade, o login, os grants, o papel e os workspaces que o operador opera da própria autenticação, nunca de argumentos; não encaminha credencial alguma e trata tudo o que o agente envia como não confiável. Devolve o manifesto canônico, um diff, diagnósticos e a base, e não grava nada: nenhum rascunho, nenhuma versão, nenhum ponteiro movido, nenhum registro de auditoria. Uma pessoa cola a proposta no console, que a valida de novo e reconfere a base, a autoridade e os grants ao salvar e aplicar.

### Rodar o servidor de propostas

Acrescente `[proposals]` ao manifesto de bootstrap que o console e o job leem (ele exige `[workspaces]`):

```toml
[proposals]
public_url = "https://proposals.example.com"  # https, salvo em loopback, sem caminho
# audience = "https://proposals.example.com"  # o aud dos seus tokens, public_url por padrão
```

Rode `meada workspaces mcp --listen 0.0.0.0:8090 --workspace meada.workspace.toml` em um processo próprio, ao lado do console e nunca em um nó. Ele lê o que o console lê: o Postgres (toda sessão somente leitura), os documentos de versão do store, as gerações que seu `[snapshot] local_dir` mantém (mantenha um `meada snapshot pull` rodando para ele como para o console, ou a configuração efetiva mostra todo repositório como pendente) e os metadados e as chaves do emissor. Ele nunca chama o GitHub, um nó ou a API do provedor de identidade, e não guarda chave de GitHub App. O endpoint MCP é a raiz de `public_url`, e os metadados de recurso protegido ficam em `/.well-known/oauth-protected-resource`. Registre `[proposals] audience` como recurso no provedor de identidade: um token de nó ou do console é recusado aqui (401), e o token deste servidor é recusado por eles.

| Ferramenta | O que responde |
| --- | --- |
| `catalog` | Os repositórios que quem chama pode selecionar (`query`, `limit`, `after`), como o catálogo do console: `state`, `reason`, a idade do conjunto e uma página de identidades. |
| `workspaces` | Os workspaces que quem chama pode ver, com `seq` (a base) e se quem chama opera cada um. |
| `show` | Um workspace visível: seu registro, `seq`, as entradas que quem chama lê e o texto de manifesto delas. |
| `propose` | `entries` (texto de manifesto), `workspace` (o workspace a mudar, se houver), `name`, `description` e `hints`: a proposta, abaixo. |

Uma proposta é `{base, manifest, diff, diagnostics, eligibility, refusals, effective, applicable}`. `base` é `{workspace, seq, version}`, nula para um workspace novo: o que o salvamento do console ainda precisa encontrar. `manifest` é o texto canônico das entradas que quem chama lê, sem anotação de visão; `diff` o compara com as entradas da base que quem chama lê; `diagnostics` são os problemas de validação nas palavras da CLI, com linha, entrada e chave quando os nomeiam; `eligibility` dá cada identidade nomeada como `selectable`, `unavailable` ou `pending`; `refusals` são os erros que o console responderia agora (`not_operator`; `entries_refused`, nomeando só o que quem chama lê e contando o resto em `hidden`; `pending` com o motivo); `effective` é a configuração efetiva sobre a base; `applicable` diz se o console a aceitaria agora. Toda ferramenta é somente leitura, e nenhuma tem argumento que nomeie ator, papel, grant ou login: um assim é recusado como argumento inválido. As entradas são limitadas (corpo de 80 KiB, manifesto de 64 KiB, 50 hints com notas de 256 caracteres, páginas de 100), e uma resposta acima de 192 KiB é `too_large`, nunca cortada.

### A sessão de um agente

1. Configure dois servidores MCP no agente: o MCP de leitura do GitHub que sua organização já autoriza, com as próprias credenciais, e o servidor de propostas do Meada, com um token para `[proposals] audience` obtido do seu provedor de identidade. O Meada nunca recebe as credenciais do GitHub, e o MCP do GitHub nunca recebe o token do Meada.
2. Pergunte a `catalog` o que o operador pode selecionar, e a `workspaces` e `show` o que já existe.
3. Leia os repositórios candidatos com o MCP do GitHub: rotas, clientes, pacotes, qual repositório serve qual contrato. O que ele diz sobre acesso é uma pista, não um grant.
4. Chame `propose` com o texto do manifesto, o `workspace` a mudar, se houver, e os repositórios lidos como `hints`. Uma pista nunca acrescenta uma entrada nem muda um resultado, e a nota dela é descartada sem ser lida.
5. Corrija o que `diagnostics` e `refusals` apontam e proponha de novo até `applicable` ser verdadeiro. Chamadas iguais dão respostas iguais.
6. Entregue o `manifest` e a `base` ao operador para revisão no console. Para uma mudança, o operador cola o texto no editor do workspace, confere a configuração efetiva e o diff, e salva e aplica nessa base. Para um workspace novo, o operador o cria a partir do catálogo com os mesmos repositórios e depois cola o texto no editor dele. Se alguém mudou o workspace nesse meio tempo, o console recusa a base velha e o agente propõe de novo.

A separação entre [versão de workspace e geração](workspaces.md), e entre [admissão e grant](admission.md), continua essencial para ler este desenho.

## Ordem de entrega

Contratos, depois o catálogo elegível, o primeiro workspace, versões e sobreposição, e a configuração por contexto. A autoria guiada e as propostas por MCP avançam então em paralelo, e a validação empresarial depende de ambas. O [roadmap](roadmap.md) acompanha cada etapa.
