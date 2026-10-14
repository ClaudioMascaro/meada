# Security policy

[Português (Brasil)](SECURITY.md) · **English** · [Español](SECURITY.es.md)

> This is a translation. The Brazilian Portuguese [policy](SECURITY.md) is the source; if they
> disagree, it wins, and a fix to this page is welcome.

## Supported versions

Only the **latest published 0.x version** receives security fixes. Until Meada reaches 1.0,
update to the latest version before reporting, if you can.

## How to report a vulnerability

**Do not open a public issue.** Use GitHub's private vulnerability reporting:

1. Open the repository's [Security](https://github.com/ClaudioMascaro/meada/security) tab.
2. Choose **Report a vulnerability**.
3. Describe the problem: the Meada version, the operating system, the steps to reproduce and
   the impact you see.

You may write in Portuguese, Spanish or English.

## What happens next

The maintainer acknowledges receipt, assesses the report and talks to you through the private
report itself. When there is a fix, it ships in a new 0.x version, and the GitHub security
advisory credits whoever reported it, if you wish.

## What matters

For example: indexing or the MCP server writing outside Meada's own index, reading files outside
the indexed repository, running code that comes from the indexed repository, or exposing the MCP
server beyond the local machine. If you are unsure whether something is a vulnerability, report
it anyway.
