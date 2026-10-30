# Política de segurança

**Português (Brasil)** · [English](SECURITY.en.md) · [Español](SECURITY.es.md)

> Esta é a versão original. As traduções para inglês e espanhol seguem o mesmo sentido; se
> divergirem, vale esta página.

## Versões com suporte

Só a **última versão 1.x publicada** recebe correções de segurança. Atualize para a última
versão antes de reportar, se puder.

## Como reportar uma vulnerabilidade

**Não abra uma issue pública.** Use o relato privado de vulnerabilidades do GitHub:

1. Abra a aba [Security](https://github.com/ClaudioMascaro/meada/security) do repositório.
2. Escolha **Report a vulnerability**.
3. Descreva o problema: a versão do Meada, o sistema operacional, os passos para reproduzir e o
   impacto que você enxerga.

Você pode escrever em português, español ou English.

## O que acontece depois

O mantenedor confirma o recebimento, avalia o relato e conversa com você pelo próprio relato
privado. Quando houver correção, ela sai numa nova versão 1.x (patch ou minor), e o aviso de segurança do GitHub
dá o crédito a quem reportou, se você quiser.

## O que interessa

Por exemplo: a indexação ou o servidor MCP escreverem fora do índice do próprio Meada, lerem
arquivos fora do repositório indexado, executarem código vindo do repositório indexado, ou
exporem o servidor MCP além da máquina local. Se estiver em dúvida se algo é uma vulnerabilidade,
reporte do mesmo jeito.
