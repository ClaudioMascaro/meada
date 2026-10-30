# Installation

Pick one of five ways.

- **Cargo**: `cargo install meada`.
- **Prebuilt binary**: download the file for your system from the
  [GitHub releases](https://github.com/ClaudioMascaro/meada/releases) (Linux, macOS and
  Windows, on x86_64 and arm64) and put `meada` on your `PATH`.
- **Installer** (Linux and macOS):
  `curl --proto '=https' --tlsv1.2 -LsSf https://github.com/ClaudioMascaro/meada/releases/latest/download/meada-installer.sh | sh`.
- **npm**: `npx @meada/meada <command>` downloads the prebuilt binary for your platform and runs
  it, with nothing to install first.
- **Container**: `ghcr.io/claudiomascaro/meada:<version>` for the default build, or
  `<version>-cloud` with every cloud module, both for Linux amd64 and arm64. `latest` and
  `latest-cloud` follow the latest stable release; prereleases get version tags only.
  The Debian bookworm-slim base (git and CA certificates) is refreshed only with a Meada release.
  The image runs without root and defines no command: pass the arguments explicitly, for
  example `docker run --rm ghcr.io/claudiomascaro/meada:latest --version`. To serve, mount
  the manifest, mirrors and indexes, and pass
  `mcp --listen 0.0.0.0:7333 --workspace /etc/meada/meada.workspace.toml`, following
  [Server mode](server.md) and its admission configuration.

For the console, run `cargo install meada --features ui`, download
`meada-ui-<version>.tar.gz` from the same release and unpack it. Point `[ui] dir` in the
manifest at the extracted `meada-ui-<version>/` directory. The console refuses to start with
a UI of another version and names both versions. The `-cloud` image already carries these
files at `/usr/share/meada/ui`; the default image and prebuilt binaries still carry no UI.
Node is needed only to build the frontend from source, never to install through Cargo or
run the console.

To index Java and Kotlin, only the Meada binary and git are needed. No JDK, Kotlin compiler, Maven or Gradle is required; analysis reads `.java`, `.kt` and `.kts` files without compiling or running the project. C# needs no .NET SDK or MSBuild either: Meada reads the `.cs` and `.csproj` files.
