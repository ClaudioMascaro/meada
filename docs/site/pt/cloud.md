# Módulos de nuvem

Módulos opt-in, desligados por padrão e documentados abaixo. Um módulo é
uma feature do cargo mais uma seção de `meada.workspace.toml`; os binários de release são o build
padrão, sem nenhum, e uma seção num build sem a sua feature é um erro que nomeia a feature. A
feature `cloud` liga todos de uma vez (`cargo install meada --features cloud`) e, como
`refresh-lock`, só compila em Unix.

Escolha os módulos conforme a operação. Cada um é opt-in: a feature precisa estar no binário e a seção correspondente no manifesto.

- [Snapshots](snapshots.md)
- [Gerações e retenção](generations.md)
- [Refresh e publicação](refresh.md)
- [Autenticação e grants](admission.md)
- [Credenciais de GitHub App](github-app.md)
- [Console de gerenciamento](console.md)
- [Identidade empresarial: Postgres, auditoria, grant do GitHub e diretório](enterprise.md)
