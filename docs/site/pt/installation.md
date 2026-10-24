# Instalação

Esta árvore declara **0.1.0** e prepara a 1.0. Os canais abaixo descrevem a instalação de
uma versão publicada. A geração do instalador npm está configurada, mas sua publicação e
verificação ainda são etapas pendentes desta preparação; os exemplos `npx` pressupõem isso.

Escolha uma das cinco formas e confirme a versão com `meada --version` ou `npx meada --version`.

- **Cargo**: `cargo install meada`.
- **Binário pronto**: baixe o arquivo do seu sistema nas
  [releases do GitHub](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS e
  Windows, em x86_64 e arm64) e ponha `meada` no `PATH`.
- **Instalador** (Linux e macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx meada <comando>` baixa o binário pronto da sua plataforma e o executa, sem
  instalar o Meada antes. É preciso ter npm/npx disponível. O pacote acompanha a versão do
  binário e o baixa da mesma release do GitHub; veja a [configuração MCP](mcp.md) para agentes.
- **Contêiner**: `ghcr.io/claudiomascaro/meada:<versão>` para o build padrão, ou
  `<versão>-cloud` com todos os módulos cloud, ambos para Linux amd64 e arm64. `latest` e
  `latest-cloud` acompanham a última release estável; pré-releases só têm tags de versão.
  A base Debian bookworm-slim (git e certificados CA) só é atualizada numa release do Meada.
  A imagem roda sem root e não define comando: passe os argumentos explicitamente, por
  exemplo `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. Para servir,
  monte o manifesto, os espelhos e os índices, e passe
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, seguindo
  [Modo servidor](server.md) e sua configuração de admissão.

Para compilar o clone atual, na raiz execute `cargo install --path . --locked`.
O build padrão serve MCP local por stdio e HTTP. Recursos opcionais são habilitados por
features do Cargo, por exemplo `cargo install --path . --locked --features cloud` para
todos os módulos. A [referência de comandos](cli.md) identifica a feature de cada comando.

Para a console, execute `cargo install meada --features ui`, baixe o
`meada-ui-<versão>.tar.gz` da mesma release e descompacte-o. Aponte `[ui] dir` no manifesto
para o diretório `meada-ui-<versão>/` extraído. A console recusa iniciar com uma UI de outra
versão e informa as duas versões. A imagem `-cloud` já contém esses arquivos em
`/usr/share/meada/ui`; a imagem padrão e os binários prontos continuam sem a UI. Node só é
necessário para compilar o frontend a partir dos fontes, nunca para instalar pelo Cargo ou
executar a console.
