# Como contribuir com o Meada

**Português (Brasil)** · [English](CONTRIBUTING.en.md) · [Español](CONTRIBUTING.es.md)

> Esta é a versão original. As traduções para inglês e espanhol seguem o mesmo sentido; se
> divergirem, vale esta página.

Obrigado por querer ajudar. Este guia diz como montar o ambiente, como propor uma mudança e o
que o projeto espera de você. Ao participar, você concorda em seguir o
[Código de Conduta](CODE_OF_CONDUCT.md).

## Em que língua escrever

- **Issues, PRs e revisões** podem ser escritos em português, español ou English. Respondemos
  na língua que você usou.
- **Código, identificadores, comentários, mensagens de commit e ADRs** são em inglês, para que
  qualquer pessoa consiga ler.
- **A documentação para pessoas** (README, este guia, o código de conduta) nasce em português do
  Brasil. As versões `.en.md` e `.es.md` são traduções, nessa ordem de prioridade. Se você muda
  um arquivo em português, atualize as duas traduções no mesmo PR; se não puder, abra uma issue
  com o rótulo `translation` para cada uma que ficar para trás.
- Achou um trecho estranho no inglês ou no espanhol? Corrigir a tradução é uma ótima primeira
  contribuição.

## Antes de começar

- Procure uma issue aberta que já cubra a sua ideia. Se não houver, abra uma. Para mudanças
  grandes, converse na issue antes de escrever código.
- Para uma conclusão errada do Meada (drift, ligação ou nível), use o formulário `inference`, ao lado
  dos de bug e de recurso; troque nomes privados por marcadores como `repoA` e `fn1`.
- As issues com o rótulo `good first issue` são pensadas para quem está chegando, e `help
  wanted` marca onde a ajuda faz mais falta.
- O escopo da v1 e a ordem do trabalho estão no
  [ADR 0003](docs/adr/0003-v1-scope-and-roadmap.md).
- Use os termos do [`GLOSSARY.md`](GLOSSARY.md) no código, nos docs e nas issues: *repository*,
  *index*, *peer*, *provider*, *consumer*, *contract*, *link*, *level*, *chain* e os demais.
- As decisões do projeto ficam em [`docs/adr`](docs/adr). Uma mudança que contraria uma delas
  começa com um novo ADR. Em especial, os invariantes do produto (sem LLM nem rede na
  indexação, local primeiro, sem ferramentas de escrita no MCP, um índice por repositório,
  determinismo) estão no [ADR 0002](docs/adr/0002-product-invariants.md).

## Regra de clean room

Meada cobre um terreno que o GitNexus (licença PolyForm Noncommercial) também cobre. Para não
contaminar o projeto:

- construa a partir dos ADRs e das specs do próprio Meada;
- não copie nem leia o código-fonte do GitNexus, o `dist/` do pacote npm nem as constantes
  ajustadas dele, e não cole nada disso em issues, PRs ou commits.

## Montar o ambiente

Você precisa do Rust. O arquivo `rust-toolchain.toml` fixa a versão do compilador e instala
`rustfmt` e `clippy` sozinho quando você usa o [rustup](https://rustup.rs).

```sh
git clone https://github.com/ClaudioMascaro/meada.git
cd meada
cargo build
cargo test
```

## Antes de abrir o PR

Estes três comandos são os mesmos que a CI roda no build padrão (`fmt`, `clippy`, `test`), e
os três precisam passar:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Uma mudança sob a feature de um módulo de nuvem `<f>` (ADR 0012) também passa por
`cargo clippy --all-targets --features <f> -- -D warnings` e `cargo test --features <f>`. A CI
tem ainda o job `cloud`: confere que o build padrão não traz nenhum crate de módulo e roda o
clippy e os testes com `--all-features`, contra um Valkey e um servidor compatível com S3 (os
testes que precisam deles leem `MEADA_TEST_VALKEY_URL` e `MEADA_TEST_S3_ENDPOINT`, e sem elas
avisam e pulam).

Se a mudança mexe em comportamento, inclua o teste que a cobre.

## Documentação do site

As páginas ficam em `docs/site/pt`, `docs/site/en` e `docs/site/es`, com o mesmo nome nos três
idiomas. O [guia de build](docs/engineering/documentation-build.md) explica a instalação das
dependências, a prévia local e a publicação. Antes de enviar uma alteração, rode
`mkdocs build --strict` e `python scripts/docs/check.py` no ambiente de documentação. Esses
comandos conferem links, âncoras, traduções e diagramas, além dos gates Rust acima.

## Commits e PRs

- **Conventional Commits**: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `chore:`, `ci:`, com
  escopo opcional. Exemplo: `fix(resolve): keep the link level when a route has a prefix`.
- Nomeie a branch pelo tipo: `feat/...`, `fix/...`, `docs/...`.
- Abra o PR contra `main` e preencha o modelo. Ligue o PR à issue (`Closes #123`).
- O PR é integrado por **squash merge**: a branch vira um único commit em `main`, e o título do
  PR vira a mensagem desse commit. Por isso o título do PR também segue Conventional Commits.
- Os checks `fmt`, `clippy` e `test` precisam estar verdes antes da integração.
- Mantenha o PR pequeno e com um propósito só. Mudanças sem relação ficam em PRs separados.

## Segurança

Não abra issue pública para vulnerabilidade. Veja a [política de segurança](SECURITY.md).
