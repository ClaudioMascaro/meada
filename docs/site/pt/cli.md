# Comandos

Execute os comandos locais dentro do repositório que quer consultar:

```sh
meada index
meada status
meada export > export.json
meada mcp
meada decide --list
```

`index` monta o índice local em `.meada/` para o repositório e seus pares. `status` mostra o
workspace, os índices, pares ausentes, recursos sem ligação, `via` sem símbolo e decisões
obsoletas ou à espera de um par. `export` produz o [JSON canônico](export.md) de um
repositório; `mcp` serve as [três ferramentas](mcp.md); `decide` trata [decisões](decisions.md).

Consultas locais atualizam primeiro os índices com os arquivos atuais, inclusive mudanças
não commitadas. Cada repositório tem seu índice em `.meada/`, ignorado pelo git.
`decide` pode gravar `meada.decisions.toml` na raiz. Em modo servidor, os índices ficam no
`index_dir` do manifesto, consultas não os atualizam e os comandos operacionais têm seus
próprios efeitos descritos nos guias abaixo.

## Build padrão

Nesta tabela, `PATH` é um caminho, `ID` uma identidade de repositório e `ADDRESS` um endereço
com porta. Flags booleanas começam desligadas; opções não passadas ficam ausentes, salvo
os padrões indicados. Não existe caminho padrão para `--workspace` nem porta padrão.

| Comando | Opções e argumentos | Padrão e restrições |
| --- | --- | --- |
| `index` | `--workspace PATH` | Ausente: workspace local descoberto. Presente: indexa os repositórios listados no manifesto. |
| `status` | `--workspace PATH` | Ausente: estado local. Presente: estado do manifesto. |
| `export` | `--workspace PATH`, `--repo ID` | O par é opcional, mas cada opção exige a outra; sem ambas, exporta o repositório atual. |
| `mcp` | `--listen ADDRESS`, `--workspace PATH` | O par é opcional, mas cada opção exige a outra; sem ambas, stdio local. Com ambas, MCP por HTTP em modo servidor. |
| `decide` | `--item ID`, `--dry-run`, `--list`, `--prune`, `--decider`, `--show-request`, `--min-p FLOAT` | Sem flags, escolha humana para os itens abertos. `--prune` conflita com `--item`, `--dry-run` e `--list`; `--show-request` e `--min-p` exigem `--decider`. |

`decide --item ID` trata só uma chamada, inclusive uma já decidida; `--dry-run` imprime o
arquivo proposto; `--list` lista itens/candidatos e decisões obsoletas sem perguntar;
`--prune` remove as obsoletas, preservando as que esperam um par. `--decider` habilita o
modelo por pedido explícito. `--show-request` só mostra a requisição, sem enviá-la.
`--min-p` aceita número finito entre 0 e 1, inclusive, com padrão **0.5**.

`ADDRESS` aceita `IP:porta`, `[IPv6]:porta`, `localhost:porta`, `:porta` ou só `porta`.
Sem host, usa `127.0.0.1`; não resolve nomes DNS. A porta cabe em um inteiro sem sinal de
16 bits. Exemplo: `meada mcp --listen 7333 --workspace meada.workspace.toml`.
Veja [Modo servidor](server.md) antes de expor uma interface além do loopback.

## Módulos opcionais

A coluna feature indica o build que inclui cada comando/opção. `--features cloud` reúne os
módulos; também é possível habilitá-los individualmente pelo Cargo. `oidc` e a infraestrutura
`valkey` não acrescentam comandos. Os grupos `snapshot`, `github-app`, `workspace`,
`snapshot pointer` e `snapshot fence` exigem um subcomando.

Em todas as linhas abaixo, `--workspace PATH` é **obrigatório**. IDs de geração e de versão
são 16 dígitos hexadecimais minúsculos; não são nomes de branches.

| Comando/opção | Feature | Demais argumentos, opções e padrões |
| --- | --- | --- |
| `status --generations` | `snapshot` | Exige `--workspace`; consulta gerações em vez dos índices locais. |
| `export --generations` | `snapshot` | Exige `--workspace` e, pelo par de export, `--repo ID`. |
| `mcp --generations` | `snapshot` | Exige `--listen ADDRESS` e, por ele, `--workspace`; serve gerações. |
| `snapshot publish` | `snapshot` | Publica a geração dos repositórios do manifesto. |
| `snapshot pull` | `snapshot` | `--every SECONDS` opcional, inteiro sem sinal de 64 bits, mínimo 1; sem ele faz uma rodada, com ele repete, inclusive após uma rodada que falhou. |
| `snapshot lock` | `refresh-lock` | `-- COMMAND...` obrigatório, com pelo menos um argumento; executa esse comando sob o lock. |
| `snapshot pointer set GENERATION` | `refresh-lock` | Faz uma geração retida virar a atual. |
| `snapshot pointer retain GENERATION` | `refresh-lock` | Retém uma geração cujo documento já existe no store. |
| `snapshot pointer drop GENERATION` | `refresh-lock` | Remove a retenção; recusa a geração atual. |
| `snapshot fence recover` | `refresh-lock` | `--at-least FENCE` opcional: só dígitos decimais, ausente significa piso 0. Leitura/transporte aceitam `u64`, mas a recuperação recusa pisos ou fences registrados acima de `9223372036854775806`, preservando uma aquisição do `INCR` assinado. Eleva para o máximo observado/piso, nunca reduz. |
| `github-app credential ACTION` | `github-app` | `ACTION` obrigatório: `get`, `store` ou `erase`; lê o protocolo de credenciais do git em stdin. `store` não faz nada; `erase` invalida o token em cache correspondente. |
| `github-app throttled REPOSITORY` | `github-app` | URL HTTPS obrigatória; `--retry-after SECONDS` opcional, inteiro sem sinal de 64 bits. Sem ele começa em 60 s e dobra enquanto a marca anterior vigora; duração efetiva limitada a 1..3600 s. |
| `ui` | `ui` | `--listen ADDRESS` obrigatório; serve a console com os arquivos configurados no manifesto. |
| `workspace import FILE` | `generation-pointer` | `FILE` e `--message TEXT` obrigatórios; `--author TEXT` opcional, padrão `cli:<login>@<host>`. Salva uma versão imutável. |
| `workspace list` | `generation-pointer` | Lista versões, mais novas primeiro, marcando a atual. |
| `workspace show [VERSION]` | `generation-pointer` | Sem `VERSION`, mostra a versão atual. |
| `workspace apply VERSION` | `refresh-lock` | `VERSION` obrigatório; faz essa versão valer no próximo refresh. |
| `workspace pull` | `generation-pointer` | Baixa a versão atual e atualiza sua cópia local em `<index_dir>/workspace` (documento JSON e `current`), usada na leitura do workspace gerenciado. Não edita o manifesto TOML/bootstrap nem faz clone ou fetch. |

Os procedimentos e validações de operação estão em [Snapshots](snapshots.md),
[Gerações e retenção](generations.md), [Refresh e publicação](refresh.md),
[Credenciais de GitHub App](github-app.md), [Versões e owners](workspaces.md) e
[Console](console.md).

## Ajuda, versão e idioma

`-h`/`--help` existe em cada nível; `help [COMMAND]...` mostra ajuda de um comando, por
exemplo `meada help snapshot pull`. Na raiz, `-V`/`--version` mostra a versão do binário.
As mensagens da CLI seguem `LANG` (português ou inglês); as respostas MCP são sempre em
inglês. Para conferir o inventário em um build, use a ajuda desse binário: comandos opcionais
só aparecem quando a feature correspondente foi compilada.
