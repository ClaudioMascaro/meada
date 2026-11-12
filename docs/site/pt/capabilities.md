# Capacidades e limites

O Meada 1.4 indexa TypeScript, JavaScript, Python, Rust, Go, Java, Kotlin, C# e Swift. Ele encontra chamadas no código e liga repositórios por HTTP, pacotes e canais de mensagens. Os resultados indicam o nível de certeza de cada ligação.

O modo servidor inclui [console](console.md), [snapshots](snapshots.md), [retenção](generations.md) e [workspace gerenciado](workspaces.md), em módulos opcionais. A console tem editor, diff, playground e grafo, e um workspace pode delegar suas entradas a [times](console.md#times). Com `accounts`, a console autentica [contas locais](console.md#contas-locais) sem provedor de identidade: a conta padrão `admin` troca a senha no primeiro login, senhas comuns são recusadas, dez falhas seguidas bloqueiam a conta, `meada ui reset-admin` recupera a conta padrão e o operador global cria contas, muda o papel, suspende, reativa e redefine a senha delas, sem nunca deixar a console sem operador ativo, e dá a elas grupos que `[accounts.groups]` liga a repositórios, limitando o que cada conta lê e tornando operador de time quem é dono de entradas; ao lado de um provedor de identidade, `break_glass = true` mantém só a conta padrão como [break glass](console.md#break-glass). Uma conta com sessão aberta gera [tokens de agente](console.md#tokens-de-agente) para os nós e, com `proposals`, para o servidor de propostas, válidos de 1 a 168 horas e mostrados uma vez só, que o `meada mcp --listen` e o `meada workspaces mcp` com `[accounts]` admitem e responde sob a concessão da conta, sem bearer compartilhado. Com `[accounts.github]`, a pessoa [verifica o seu login do GitHub](console.md#login-do-github-verificado) uma vez, colando um token de acesso pessoal que a console usa só para ler o login, e os seus papéis no GitHub passam a lhe conceder repositórios na console e nos nós. O [catálogo de repositórios](console.md#catalogo-de-repositorios) lista, antes de existir um manifesto, só o que a pessoa pode selecionar. [Compose](https://github.com/ClaudioMascaro/meada/blob/main/deploy/compose/) e [Helm](https://github.com/ClaudioMascaro/meada/blob/main/deploy/helm/meada/) trazem exemplos de implantação. Os [workspaces nomeados](named-workspaces.md) de `workspaces` podem ser mantidos como versões (salvar, aplicar, reverter, renomear, apagar, delegar) por seus operadores, pelo console e por `meada workspaces`, com cada mudança conferida contra a base que leu. Cada workspace pode declarar os seus próprios peers, referências a variáveis de ambiente, recursos e hosts de serve para um repositório compartilhado, e o console mostra a [configuração efetiva](named-workspaces.md#configuracao-efetiva) (o que o `meada.toml` do próprio repositório substitui, o que ainda não vale) antes de o texto ser salvo. Os operadores montam esse manifesto com [controles guiados](named-workspaces.md#autoria-guiada) (repositórios do catálogo, peers, configurações de serve, recursos, nomes de variáveis de ambiente) que continuam sendo um só documento com o texto, mostrando o diff e congelando o conteúdo revisado até confirmarem. Com `proposals`, um [servidor de propostas](named-workspaces.md#autoria-assistida-por-mcp) (`meada workspaces mcp`) deixa um agente de CLI, com o próprio MCP de leitura do GitHub, listar o que o operador pode selecionar e validar um manifesto como o console o salvaria, sem gravar nada, para uma pessoa revisar e aplicar no console. Os workspaces nomeados e o servidor de propostas também funcionam com contas locais, com `[accounts.github]` no lugar de `[oidc.github]`.

O [grafo da console](console.md#infraestrutura-no-grafo) exibe os provisionadores e suas raízes,
as ligações de provisionamento, construtor → executor de imagens e emissor → correspondente de
alertas. A legenda explica os papéis; tabelas com nomes acessíveis permitem navegar por teclado,
mantêm nível e motivo das ligações e mostram lados sem contraparte. O playground oferece
buscas `value`, `iac`, `image` e `alert`.

## Linguagens e resolução

| Linguagem | Arquivos lidos |
| --- | --- |
| TypeScript e JavaScript | `.ts`, `.tsx`, `.js`, `.jsx`, `.mjs`, `.cjs` |
| Python | `.py`; cada `pyproject.toml` dá raízes de código (seu diretório, as raízes declaradas ou `src`) aos arquivos abaixo dele |
| Rust | `.rs` alcançados pela árvore de módulos de cada crate |
| Go | `.go` incluídos pelas regras de build do pacote |
| Java | `.java`, resolvidos pelas declarações de `package` |
| Kotlin | `.kt` e `.kts` (fora `build.gradle.kts` e `settings.gradle.kts`), resolvidos pelas declarações de `package` |
| C# | `.cs`, agrupados em projetos pelos `.csproj` no estilo SDK e resolvidos pelos `namespace` |
| Swift | `.swift` (fora `Package.swift` e `Package@swift-<versão>.swift`), agrupados em módulos pelos targets do `Package.swift` |

Uma anotação (Java, Kotlin), um atributo (C#, ou um atributo de rota do Rust) ou um decorador (Python) escrito sobre uma definição é uma chamada do escopo que a envolve, no nível em que resolve. O ponto de chamada do export nomeia a definição que ela decora (`decorates`), e o `context` a lista à parte, em `annotations:`, e não entre os callees.

### TypeScript e JavaScript

O Meada lê funções, classes e métodos, bindings `const`, `let` e `var` de função ou classe, e `export default`. Lê também funções definidas por atribuição no topo do arquivo: `obj.y = function () {}` define `obj.y`, `X.prototype.y = function () {}` define o método `X.y`, `exports.y = ...` e `module.exports.y = ...` definem `exports.y`, exportado como `y`, e `module.exports = function () {}` define o export padrão. Um `let` ou `var` só é definição se o arquivo nunca atribuir esse nome de novo. Funções e métodos do objeto literal de um `const` no topo definem `o.m`; objetos de opções escritos como argumento mantêm as definições inline. Uma chamada `obj.y()` ou `y()` no mesmo arquivo alcança essas definições, exceto `exports.y`, que um `y()` solto não alcança (o Node não cria um `y` local). `require` e `module.exports` não são imports entre arquivos, e `new F()` de uma função construtora não alcança os métodos de `F.prototype`. Um import usa os `paths` e o `baseUrl` do `tsconfig.json` (ou `jsconfig.json`) mais próximo, na pasta do arquivo ou acima; ambos são relativos à pasta desse config, e `node_modules` é ignorado. Se esse config tem `extends` para outro `tsconfig.json` ou `jsconfig.json` do repositório (caminho relativo), `paths` e `baseUrl` vêm da cadeia, o mais próximo que declara cada opção vence; um `extends` para pacote ou para fora do repositório não é lido, e uma cadeia longa demais ou circular vale só pelo config mais próximo.

### Rust

Cada `Cargo.toml` com `[package]` define um crate, em qualquer pasta. O Meada segue seus `mod` sem executar o toolchain Rust. Lê funções, tipos, traits, módulos e métodos de `impl`. Um arquivo que nenhum `mod` alcança fica de fora.

Chamadas como `crate::a::f()`, `Tipo::new()` e `Self::f()` seguem o caminho escrito no código. A resolução também acompanha `use`, grupos, renomeações, globs, `pub use` e outros crates do workspace pelo nome.

Para `x.m()`, o Meada usa o tipo do receptor quando ele aparece em um `let`, parâmetro, campo, `self` ou retorno de função. Uma chamada sobre o resultado de outra (`Router::new().route(..)`, também após `.await`, `?` ou parênteses) resolve pelo tipo de retorno escrito da chamada interna: após `?`, o tipo que um `Result<T>` ou `Option<T>` envolve, e após `.await` numa função que retorna `impl Future<Output = T>`, `T`. Um `dyn Trait` ou genérico limitado por trait chega ao método do trait. Sem tipo escrito, a resolução usa o nome. Métodos ausentes dos `impl` do tipo, como os gerados por `derive` ou vindos de traits externos, não geram ligação. Atributos de rota também não ligam a funções.

A análise usa a configuração padrão de cada crate para x86_64 Linux. Itens com `#[cfg(..)]` falso e features fora do `default` ficam de fora; módulos compilados em um build de teste entram, não só os `#[cfg(test)]`. Chamadas dentro de macros não são indexadas, com uma exceção: o corpo entre chaves de uma macro de itens, invocada num módulo ou entre os itens de um `impl` ou de um trait, é lido como itens desse módulo ou itens associados desse bloco (funções, tipos, `impl`, `use`, `mod` e as chamadas dentro deles); a invocação é escrita `m!` ou `crate::m!`, e qualquer outro caminho é ignorado. Macro de itens é um `macro_rules!` do próprio repositório cujo primeiro braço recebe `$($x:item)*` e devolve cada item só com atributos externos antes dele, como o `cfg_rt!` do tokio; os `#[cfg(..)]` que o braço escreve valem para cada item. Se o nome tem várias definições no repositório, todas precisam concordar nos atributos. Outras macros, inclusive as de outros crates, continuam sem leitura. Os contratos Rust cobrem pacotes, HTTP e canais.

### Go

O AWS SDK for Go v2 reconhece clientes criados por `New` ou `NewFromConfig`, inclusive aliases e fábricas seguidas por chamadas exatas. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM e Secrets Manager seguem as formas da seção de SDKs AWS abaixo. Um tipo declarado sozinho não comprova a origem do cliente, exceto um cliente SQS declarado `*sqs.Client`, que mantém as formas de SQS que o Meada já lia.

O caminho `module` de cada `go.mod` resolve imports para pastas de pacotes, incluindo `replace` para uma pasta do repositório. Um módulo aninhado controla sua subárvore. Arquivos na mesma pasta com o mesmo `package` compartilham escopo, inclusive `_test.go`. O Meada não executa o toolchain Go.

A análise usa linux/amd64. Arquivos `_windows.go`, com `//go:build` falso ou com cgo (`import "C"`) ficam de fora. O Meada lê funções, tipos, interfaces e métodos (`Tipo.Metodo`). Resolve chamadas no pacote e por imports (`pkg.F()`, alias ou ponto); biblioteca padrão e módulos externos continuam externos.

Uma chamada `recv.M()` é `exact` quando o código informa o tipo em parâmetro, campo, `var`, literal composto ou resultado de função. Sem isso, a resolução pelo nome é `inferred` ou `ambiguous`. Uma chamada por interface para no método da interface.

### Java

O AWS SDK for Java 2.x reconhece clientes síncronos e assíncronos criados por `create()` ou `builder().build()`, com pedidos construídos por builders. SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM e Secrets Manager seguem as formas da seção de SDKs AWS abaixo. Um cliente injetado só pelo tipo não comprova sua origem; fábricas precisam resolver por chamadas exatas.

O Meada lê classes, interfaces, enums, records, métodos e construtores, inclusive tipos aninhados. Os nomes incluem o pacote e, nos métodos, os tipos escritos dos parâmetros: `example.Orders.place(String,int)`. Lambdas e classes anônimas mantêm as chamadas no símbolo que as contém.

Imports simples, de pacote e estáticos, inclusive com `*`, resolvem pelas declarações de pacote. Chamadas sem receptor, com `this` ou estáticas são `exact` quando resta um método pela quantidade de argumentos, contando `varargs`; sobrecargas com a mesma quantidade ficam `ambiguous`. Tipos externos continuam externos.

Uma chamada num campo, parâmetro ou variável local de tipo escrito resolve nesse tipo e nos supertipos escritos em `extends` e `implements`, assim como `var x = new T(...)`, `var x = f()` e `f().g()` pelo tipo de retorno declarado de `f`, `super.f()` e as chamadas sem receptor ou com `this`. Cada assinatura vem da primeira classe que a declara, então `f(int)` da classe sobrepõe o da base e `f()` herdado continua visível. Uma chamada por um receptor de interface para no método da interface. Um supertipo de fora do repositório tem membros desconhecidos: com ele em qualquer ponto da hierarquia, nenhum método dela é `exact`, e a chamada resolve por nome dentro da hierarquia. A exceção são tipos da plataforma cujos membros são fixos e conhecidos (`Serializable`, `Cloneable`, `Runnable`, `Comparable`, `AutoCloseable`, `Closeable`; em Kotlin também `kotlin.Comparable`): eles escondem só os nomes que declaram, então uma classe `implements Serializable, Comparable<T>` tem chamadas `exact` às próprias sobrecargas, e só `compareTo` resolve por nome. Com o receptor desconhecido, a chamada resolve por nome: `inferred` quando um só método tem esse nome no repositório ou, fora o próprio método que chama, na hierarquia da classe que faz a chamada, `ambiguous` com todos os candidatos quando há vários.

Tipos públicos e seus membros públicos ou protegidos compõem a superfície exportada.

Um arquivo é pulado por abrangente demais quando os nomes envolventes que ele repete (cobrados em cada nó sob eles) passam de 4 MiB no total: ele fica sem símbolos nem chamadas, os outros arquivos são indexados e `meada status` o nomeia. A indexação não precisa de JDK, Maven ou Gradle e não executa código do repositório.

### Kotlin

O Meada lê classes (inclusive `data`, `sealed`, `enum` e `annotation`), interfaces, objetos, companions, funções de topo, funções de extensão, propriedades com getter e métodos, inclusive aninhados. Os nomes incluem o pacote e, nas funções, os tipos escritos dos parâmetros: `example.helper(Int)`. Um companion sem nome é `Classe.Companion`, e uma função de extensão leva o tipo do receptor: `example.Point.norm()`. As chamadas de uma lambda pertencem ao símbolo que a contém. Num método, numa função de extensão ou num bloco de escopo, uma chamada sem receptor pega a extensão do receptor implícito que está no escopo antes de uma função de topo de mesmo nome, como o Kotlin resolve (`exact`, ou `ambiguous` com a função quando a extensão pode valer para qualquer receptor); o nome de um `object` é a sua instância, então `Dispatchers.setMain(..)` alcança uma extensão importada de `Dispatchers`, enquanto o nome de uma classe mantém companion e estáticos.

Imports simples, com `as` e com `*` resolvem pelas declarações de pacote, e o próprio pacote é visível sem import; um nome que dois imports com `*` dão fica `ambiguous`. Chamadas sem receptor, com `this`, num objeto (`Registry.find()`) ou num companion (`Point.origin()`) são `exact` quando resta uma função pela quantidade de argumentos, contando parâmetros com valor padrão e `vararg`; sobrecargas com a mesma quantidade ficam `ambiguous`. Chamar o nome de uma classe a constrói: a chamada chega ao construtor primário ou a um secundário, escolhido pela quantidade de argumentos. Uma chamada sobre o resultado de outra (`f().g()`, `Box().g()`) resolve pelo tipo de retorno escrito de `f` (`Box?` e `Box<T>` dão `Box`), ou pela classe que o construtor cria; sem tipo escrito (`fun f() = Box()`) ou com tipo de fora do repositório, não é `exact`. Uma classe `expect` e as classes `actual` do mesmo nome são uma classe só, a `expect`: os membros que ela declara resolvem nela, e um membro que só um `actual` declara resolve só por nome. Dentro de uma lambda, uma chamada sem receptor ou com `this` pode ser de um receptor implícito, e resolve só por nome. Kotlin e Java do mesmo repositório se chamam pelo nome totalmente qualificado. Nomes de fora do repositório continuam externos.

Declarações que não são `private` nem `internal` compõem a superfície exportada; uma `internal` é visível em todo o repositório, e uma `private` de topo só no próprio arquivo. Um arquivo é pulado por abrangente demais quando os nomes envolventes que ele repete (cobrados em cada nó que guarda um) passam de 4 MiB no total: ele fica sem símbolos nem chamadas, os outros arquivos são indexados e `meada status` o nomeia. A indexação não precisa de compilador Kotlin, JDK ou Gradle.

### C#

Os clientes concretos dos SDKs AWS para .NET são reconhecidos por `new`, inclusive através de aliases e fábricas exatas. Pedidos com inicializadores, argumentos nomeados, listas e dicionários nomeiam os lados de SQS, SNS, EventBridge, Kinesis, S3, DynamoDB, SSM e Secrets Manager. O tipo de uma interface injetada sozinho não comprova a origem do cliente.

Cada `.csproj` no estilo SDK (com `Sdk`) é um projeto e contém os `.cs` da sua pasta, dentro de um `.sln` ou não; os `.sln` não mudam nada, e um `.csproj` antigo, sem `Sdk`, não é projeto. O Meada lê classes, records, structs e interfaces, inclusive aninhados (`Externa.Interna`) e `partial` (cada parte no seu arquivo, com os membros de todas), métodos e construtores com os tipos escritos dos parâmetros (`Acme.Orders.OrderService.Place(string,int)`) e funções locais (`Metodo.Local`). Namespaces de bloco e de arquivo dão os nomes completos. Um tipo que dois projetos do repositório declaram com o mesmo nome completo é, num arquivo de um deles, o do próprio projeto, como o compilador escolhe; num arquivo de outro projeto, é o do projeto que o seu referencia (`ProjectReference`, direta ou indiretamente), e o nome fica `ambiguous` quando os dois ou nenhum são referenciados. Um tipo `internal` (também sem modificador de acesso) só existe para os arquivos do seu projeto: num arquivo de outro projeto o nome liga ao tipo que esse arquivo vê, como se o `internal` não existisse, e uma chamada a ele não tem link; o mesmo vale para um construtor ou método `internal`, `private` ou `private protected` de um tipo público (`InternalsVisibleTo` não é lido). As pastas `obj/` e `bin/` ao lado de um `.csproj` são saída de build e ficam fora do índice, mesmo sem `.gitignore`. As chamadas de uma lambda pertencem ao símbolo que a contém, e um atributo é uma chamada no que ele decora.

`using`, `using static`, aliases (`using X = A.B`) e `global using` resolvem pelos namespaces de todos os projetos; um `global using`, os `<Using>` do `.csproj` e os implicit usings ligados (`ImplicitUsings`) valem em todos os arquivos do projeto, e só nele. Os tipos de cada namespace que envolve o do arquivo são visíveis sem `using`, do mais interno para o mais externo (em `Acme.Query.Internal`, os de `Acme.Query.Internal`, `Acme.Query` e `Acme`) e antes dos de um `using`, em namespaces de arquivo e de bloco aninhados. Chamadas sem receptor, com `this`, `base`, estáticas (`Tipo.Metodo()`) e por `using static` (que alcançam só os membros e tipos aninhados que o tipo nomeado declara, não os de suas bases) são `exact` quando resta um método pela quantidade de argumentos, contando `params` e parâmetros com valor padrão; sobrecargas com a mesma quantidade ficam `ambiguous`. Tipos de fora do repositório continuam externos. Chamadas num receptor de tipo escrito ainda não resolvem pelo tipo. Uma base de fora do repositório deixa as chamadas aos membros da classe sem `exact`, salvo `IDisposable`, `IAsyncDisposable`, `IEquatable` e `IComparable`, de membros conhecidos, que escondem só os nomes que declaram. Os itens (`<Using>`, `PackageReference`, `ProjectReference`) e as propriedades de um `Directory.Build.props` valem nos projetos abaixo dele como se estivessem no `.csproj`, avaliados antes dos deste (o `props` mais próximo esconde os de cima, a menos que importe o pai com `GetPathOfFileAbove`); um `ProjectReference` ali é relativo a cada projeto, e um construído sobre `$(MSBuildThisFileDirectory)` conta como computado. Um `props` com mais de 256 `<Using>` e referências não é lido. `nameof(x)` vale o texto do último identificador de `x` (`nameof(Pedidos.Topico)` é `"Topico"`), então um nome de canal ou uma parte de URL escritos com ele são conhecidos.

Tipos públicos e seus membros públicos ou protegidos compõem a superfície exportada. Um arquivo é pulado por abrangente demais quando os nomes envolventes que ele repete (namespace, tipos e método, a parte do nome envolvente inteiro acima de 256 bytes, cobrada em cada nó sob ele) passam de 4 MiB no total: ele fica sem símbolos nem chamadas, os outros arquivos são indexados e `meada status` o nomeia. Um projeto cujos arquivos recebem mais de 256 usings distintos (`global using`, itens `<Using>` e implicit usings juntos) tem todos os seus arquivos pulados do mesmo modo, e `meada status` nomeia cada um com o limite. Arquivos gerados com nomes comuns, como os snapshots de modelo do Entity Framework, são lidos inteiros em qualquer tamanho. A indexação não precisa de .NET SDK nem MSBuild e não executa código do repositório.

### Swift

Cada target do `Package.swift` (`.target`, `.executableTarget`, `.testTarget`, `.macro`, `.plugin`) é um módulo do seu pacote (dois pacotes do repositório com um target de mesmo nome dão dois módulos) com os `.swift` do seu `path:` literal, ou de `Sources/<nome>` por padrão; os arquivos fora de qualquer target, todos num repositório sem `Package.swift`, formam um módulo só. O Meada lê classes, structs, enums, actors e protocolos, inclusive aninhados (`Externa.Interna`), e funções, métodos e inicializadores com os rótulos dos argumentos no nome (`OrderService.place(order:quantity:)`, `init(baseURL:)`); declarações do mesmo escopo num arquivo (o corpo de um tipo e suas extensões) com os mesmos rótulos e outros tipos levam também os tipos (`audit(_:Order)`). Os membros de uma extensão são do tipo estendido, em qualquer arquivo do módulo, e os de uma extensão de protocolo são as implementações padrão dos requisitos. As chamadas de uma closure pertencem ao símbolo que a contém.

Os nomes resolvem primeiro no módulo do arquivo e depois nos módulos que ele importa (`import Store`), pelas declarações `public` e `open`, e também pelas `internal` quando o import é `@testable`; `import Store` nomeia o target do pacote do próprio arquivo ou, na falta dele, o do pacote mais próximo na árvore de diretórios (todos, quando vários estão à mesma distância, e então a chamada por ele é `ambiguous`); um módulo que não é target do repositório é externo, e uma declaração `internal` só existe no seu módulo e nos arquivos que o importam com `@testable` (as `fileprivate` e `private`, em nenhum outro). Chamadas sem receptor, com `self`, estáticas (`Tipo.metodo()`) e de inicializador (`Tipo(...)`, ou o próprio tipo quando ele não declara inicializador) são `exact` quando resta uma declaração pelos rótulos, contando parâmetros com valor padrão, variádicos e closures finais; um closure final só preenche um parâmetro de tipo função (também opcional ou com atributos como `@escaping`), de um parâmetro de tipo genérico ou de um `typealias` do repositório para um tipo função, e nunca um de um tipo que o repositório declara nem de outro tipo escrito (`Wheel?`, `[Int]`); um parâmetro escrito como um nome de fora do repositório (`animation: Animation?`, `Ext.Handler`) pode ser um alias de tipo função, então uma chamada que só cabe numa sobrecarga pelo closure final é `inferred`, ou `ambiguous` entre várias, nunca `exact` (um parâmetro antes do que o closure preenche de qualquer modo não importa); sobrecargas com os mesmos rótulos ficam `ambiguous`. Uma função com nome capitalizado é chamada como qualquer função, e uma chamada escrita sem `await` não alcança uma função declarada `async`. Uma chamada numa propriedade, parâmetro ou variável local de tipo escrito (`let x: T`, `x: T?`, `any P`, ou um `typealias` dele, como `StoreOf<X>` para `Store`, seguido de alias em alias até um limite) ou inicializada com `Tipo(...)` resolve nesse tipo, nas suas extensões e nos protocolos que ele adota, `exact` quando resta uma declaração pelos rótulos; por um receptor de tipo protocolo ela para no requisito, sem procurar os tipos que o adotam. Um nome ligado duas vezes numa função, por um parâmetro de closure ou por um padrão, ou com o nome de um parâmetro de tipo, não tem tipo conhecido ali. Uma chamada num receptor de tipo não lido resolve pelo nome e pelos rótulos (`inferred` ou `ambiguous`). Uma chamada num membro implícito (`.name(..)`, como em `wait(.seconds(1))`) resolve do mesmo modo, entre os membros e nunca para uma função ou um tipo desse nome; `.init(..)` fica sem resolução. Um receptor de tipo de fora do repositório (`session: URLSession`, `String`, ou um nome com maiúscula que nada no repositório declara, como `AF`) só alcança os membros das extensões desse tipo no repositório (`extension URLSession { }`), e uma chamada depois de um membro desse nome (`URLSession.shared.data(..)`), ou sobre um valor ligado a esse membro (`let session = URLSession.shared`), não resolve. Um receptor ou uma cabeça de caminho escrita como um `typealias` de um tipo assim (`typealias Anim = Animation`, também por alias de alias) alcança as mesmas extensões. Como em Java, um tipo com uma base de fora do repositório (`NSObject`, `View`) não tem chamadas `exact` às próprias sobrecargas, a menos que a base seja um tipo de membros conhecidos (`Sendable`, `Error`, `Equatable`, `Comparable`, `Hashable`, `Encodable`, `Decodable`, `Codable`, `Identifiable`, `CustomStringConvertible`, `CaseIterable`, o tipo do valor bruto de um enum ou o `RouteCollection` do Vapor), que esconde só os nomes que declara. Um arquivo é pulado por abrangente demais quando os nomes envolventes que ele repete (cobrados em cada nó que guarda um) passam de 4 MiB no total: ele fica sem símbolos nem chamadas, os outros arquivos são indexados e `meada status` o nomeia. A indexação não precisa de toolchain Swift nem de Xcode.

## HTTP e documentos de contrato

Rotas declaradas em IaC também são providers HTTP: API Gateway HTTP e REST no Terraform/OpenTofu (HCL e JSON), eventos SAM `Api`/`HttpApi` e Serverless `http`/`httpApi`. Métodos e caminhos literais usam a mesma normalização de parâmetros das rotas em código. O provider aponta para o símbolo do handler local quando ele resolve com certeza, ou para o repositório inteiro. Consumidores de outros repositórios usam as regras comuns de host e peers. Referências, métodos ou caminhos desconhecidos não inventam rotas; integrações que não sejam Lambda e rotas catch-all sem caminho finito não entram no mapa (#666).

URLs base vindas da configuração ou do ambiente de um workload podem nomear um Service
Kubernetes (`orders`, `orders.shop`, `orders.shop.svc` ou
`orders.shop.svc.cluster.local`, com ou sem porta) ou um serviço Compose. O Meada segue
o selector do Service e a imagem ou o caminho local do workload até o repositório que
atende a rota. Esses vínculos são `inferred`, com o Service no motivo. Nomes curtos usam
o namespace e a raiz de avaliação do workload conhecido; candidatos distintos ficam
`ambiguous`. O ambiente vem somente das entradas indexadas do workload que executa o código.

Um Ingress liga seu host e prefixo literal à rota do Service, como um prefixo de gateway.
O prefixo mais específico prevalece. Regras com anotação de rewrite, backend desconhecido
ou `pathType` diferente de `Prefix` ficam sem vínculo (a ausência de `pathType` nas formas
antigas é aceita). Caminhos de controller, expressões regulares e hosts com curinga não são
interpretados. Nenhum cluster é consultado. Quando nem o Service nem o Ingress chegam a código
(um Service que não seleciona workload indexado, uma regra sem vínculo), o host fica com o
`[serve] hosts`: um host declarado ali continua ligando em `exact`.

O Meada liga consumidores e provedores, inclusive entre linguagens e atrás de gateways. Uma rota cujo handler está num arquivo de teste (`*_test.go`, `*.test.*`, `*.spec.*`, `test_*.py`, um diretório `tests` ou `src/test`) não conta como provedora, a menos que o `meada.toml` tenha `test_routes = true` (veja [Configuração](configuration.md)):

| Linguagem | Consumidores HTTP | Provedores HTTP |
| --- | --- | --- |
| TypeScript / JavaScript | `fetch`, `discovery.getBaseUrl` e `proxy.endpoints` do Backstage (também a forma antiga, as chaves de `proxy` que começam por `/` quando nenhum arquivo tem `endpoints` nem um `$include` ali, em `proxy` ou na raiz que o Meada não segue, e o endpoint escrito como texto, `'/ep': https://...`) | Next.js app router, Express, plugins de backend do Backstage |
| Python | `requests`, `httpx`, `aiohttp` | FastAPI, Flask, Django |
| Rust | `reqwest`, `ureq` | axum, actix-web, Rocket |
| Go | `net/http`, resty | `net/http`, gin, echo, chi, gorilla/mux |
| Java | RestTemplate, WebClient, `java.net.http`, OkHttp, OpenFeign, Retrofit | Spring MVC, JAX-RS |
| Kotlin | RestTemplate, WebClient, OkHttp, Retrofit, Ktor client | Spring MVC, Ktor |
| C# | `HttpClient`, typed clients, Refit | ASP.NET Core (controllers, minimal APIs) |
| Swift | `URLSession`, Alamofire, AsyncHTTPClient | Vapor, Hummingbird |

Em TypeScript e JavaScript, um `fetch` cuja URL é montada por uma função do repositório é um consumidor quando a função só devolve uma string: uma arrow function com corpo de expressão, ou um corpo de um único `return`. A chamada (`fetch(url("api/vets"))`), ou uma constante que a guarda (`const u = url("api/owners"); fetch(u)`), dobra para a string com os parâmetros da função trocados pelos argumentos da chamada, também quando a função vem de outro módulo. Só dobra uma chamada que resolve exatamente. Uma função de mais de uma instrução, uma `async`, uma que devolve algo que não é uma string, e uma string com uma parte que não dobra (uma base lida de `process.env`, um condicional sobre uma global de build) deixam o consumidor sem resolver.

Em TypeScript e JavaScript, `new URL(x)` com um argumento é lido como `x`, como a especificação WHATWG URL define, somente para o `URL` global sem reatribuição ou o export nomeado `URL` de `url` ou `node:url` por `import` ou `require` não ocultado. Um construtor de pacote desconhecido, alias de repositório ou outro export fica opaco. `u.toString()`, `u.href` e `String(u)` repassam a URL, assim como `${u}` em uma template string, esteja a URL em uma constante ou montada no próprio lugar (`fetch(new URL(BASE + "/api/vets").toString())`), e um wrapper de cliente chamado com uma delas dobra em seus pontos de chamada. Uma base lida do ambiente mostra o seu `via`. Mudanças por `u.searchParams`, `u.search` e `u.hash` mantêm o caminho; uma escrita em qualquer outro membro de uma constante criada por `new URL` (`u.pathname = ..`, `u.host += ..`), em qualquer ponto do arquivo, deixa o consumidor sem resolver, assim como uma mutação dentro de uma função que recebe a URL, que o Meada não vê. `new URL(ref, base)` com dois argumentos é a junção dos dois, como a especificação WHATWG URL define (RFC 3986 seção 5.2 para essas referências): um `ref` absoluto é ele mesmo, `//host/x` toma o host de `ref`, `/orders` substitui o caminho da base e `orders` se junta ao diretório da base (`https://h/api/` dá `/api/orders`, `https://h/api` dá `/orders`). Sobre uma base lida do ambiente ou um buraco, `/orders` mantém a base como cabeça (`http::GET::/orders`, com `via env:API_URL` para `process.env.API_URL`); um `ref` relativo, um que começa com uma leitura, e uma query ou um fragmento sozinhos deixam o consumidor sem resolver, e um parâmetro em qualquer lado é resolvido nos pontos de chamada, como em um wrapper de cliente. `.href`, `toString()` e `String(u)` de um `new URL` de dois argumentos não são lidos.

`String(u)` respeita as declarações locais de `String` e só repassa a URL quando a chamada usa a conversão global. Atribuições e incrementos que podem substituir essa conversão deixam a URL sem resolver. Uma declaração local preserva a conversão global fora de seu escopo. Funções declaradas em blocos de JavaScript sem modo estrito também ocultam a conversão na função envolvente; módulos, classes e `use strict` preservam o escopo do bloco.

Em Java, um método de uma classe `@RestController` ou `@Controller` com `@GetMapping`, `@PostMapping`, `@PutMapping`, `@PatchMapping`, `@DeleteMapping` ou `@RequestMapping(method = RequestMethod.X)` vira provedor, sob o prefixo do `@RequestMapping` da classe. O caminho dobra constantes `static final` (também de outras classes) e `+`; cada caminho de um array é um provedor. Um `@RequestMapping` sem `method` atende todos os métodos (`http::ANY::<caminho>`) e liga a consumidores de qualquer método, com "any method" na razão. `server.servlet.context-path` não é aplicado. Anotações e `RequestMethod` valem por import do Spring, simples, estático ou `import ...bind.annotation.*`; uma classe do repositório com o mesmo nome, ou um import que também poderia trazer o nome, as esconde. As anotações também podem ser escritas por extenso, sem import (`@org.springframework.web.bind.annotation.GetMapping`), a menos que o repositório declare uma classe com esse nome. Em `method` ou num array de caminhos, um elemento ilegível é ignorado e os demais valem.

Em Java, uma classe com `@Path` (`javax.ws.rs` ou `jakarta.ws.rs`) é um recurso JAX-RS: um método com `@GET`, `@POST`, `@PUT`, `@PATCH`, `@DELETE`, `@HEAD` ou `@OPTIONS` vira provedor no caminho da classe somado ao `@Path` do método, com constantes dobradas e `{id}` ou `{id:regex}` como parâmetro. Um método só com `@Path` (localizador de sub-recurso) não é rota, e `@ApplicationPath` não é aplicado. Os nomes valem por import, como no Spring. Um controller ou recurso pode estender uma classe de fora do repositório (`extends com.corp.Base`): as anotações dos métodos são lidas pelos imports, e só um tipo aninhado dessa base com o nome da anotação seria lido errado. Uma interface com `@Path` é cliente, não servidor. Anotações escritas por extenso, sem import, são lidas; anotação herdada de interface não é lida.

Em Kotlin, os controladores Spring seguem as regras de Java, e um controller pode estender uma classe de fora do repositório. Uma rota Ktor dentro de `routing { }` (`get`, `post`, `put`, `patch`, `delete`, `head`, `options`, com caminho ou sem, e `route(caminho, HttpMethod.X)`) vira provedor sob o prefixo de cada bloco `route(prefixo) { }` em volta dela, também quando ela está numa função de extensão de `Route` ou `Routing` chamada dentro do bloco, em outro arquivo: chamada sob dois prefixos, ela serve nos dois. O lambda da rota é o handler. Os nomes do Ktor só valem com o import do Ktor; o roteamento tipado (classes de recurso) não é lido.

Em Swift, uma rota Vapor (`get`, `post`, `put`, `patch`, `delete` e `on(.METHOD, caminho)`) chamada na aplicação (`Application` ou `Application.make`) ou num `grouped(prefixo)` dela vira provedor sob o prefixo de cada `grouped` em volta, também quando o construtor de rotas passa por variáveis, argumentos e retornos de funções. Um `RouteCollection` registrado com `register(collection:)` serve as rotas do seu `boot(routes:)` sob o construtor em que foi registrado, e sob cada um quando registrado em dois. O closure final é o handler, e `use:` aceita uma função (`self.f` é o `f` do tipo em volta). Um `grouped(..)` cujos argumentos são todos chamadas num nome com maiúscula que não produz caminho (`AuthMiddleware()`, `User.authenticator()`) é a sobrecarga de middleware e não soma caminho; uma leitura de membro (`app.sessions.middleware`) deixa o grupo sem leitura, e uma chamada estática que devolve um componente de caminho (`grouped(Paths.api())`) é lida como middleware, então suas rotas são servidas sob o prefixo do pai (um falso positivo aceito), e um caminho que o Meada não lê não gera rota. Os nomes só valem com o import `Vapor`.

Em Python, `base_url_env` liga também a URL base de `os.environ`, `os.getenv` e de um campo de uma subclasse de `BaseSettings` (pydantic-settings) do mesmo arquivo: a variável é `<env_prefix><CAMPO>` em maiúsculas (o `env_prefix` literal de `model_config = SettingsConfigDict(...)` ou de `class Config`), ou o `alias` / `validation_alias` do campo, sem prefixo. O Meada nunca lê o valor de uma variável.

Em TypeScript, `base_url_env` liga a URL base de `process.env.X`, `process.env["X"]` e `import.meta.env.X`, também por fallbacks `||` e `??` e métodos de string como `trim` ou `replace`, e a de `config.getString('a.b')` ou `getOptionalString('a.b')` do Backstage cuja chave guarda exatamente `${VAR}` num `app-config*.yaml` do repositório (`app-config.yaml` primeiro, vale o primeiro arquivo que tem a chave), também quando o valor vem de um arquivo parcial por `$include` (com ou sem um caminho de chave depois de `#`, a partir da pasta do arquivo que inclui, um parcial incluindo outro) ou é um `$env: VAR`. Um `$include` com caminho absoluto, que sai do repositório, por link simbólico, com `${`, de outra extensão que `.json`, `.yaml` e `.yml`, para arquivo ou caminho de chave que não existe, com outra chave ao lado, em ciclo, além de 8 inclusões de profundidade, de 64 inclusões ou de 1 MiB enxertado a partir de um `app-config` não é seguido: as chaves sob ele ficam sem leitura e o `meada status` o nomeia com o motivo. `$file` não é lido. Um `app-config` que é um único `$include` é o arquivo que ele nomeia, e um com mais de um documento YAML não é lido, pois o Backstage o recusa. A leitura pode estar numa constante, na própria URL ou num campo de classe `readonly`, `private` ou `#x` escrito uma única vez, pelo seu inicializador ou por um `this.campo = valor` simples no corpo do construtor: `this.campo` num método é essa leitura. Um campo público mutável, um escrito duas vezes (uma escrita condicional conta como duas) ou fora do construtor, uma propriedade de parâmetro, um `this.campo = parametro` e um campo de um arquivo JavaScript continuam sem valor. O Meada nunca lê o valor de uma variável.

O `context` de uma função ou método mostra o nível, o motivo e o provedor dos vínculos `inferred` de chamadas HTTP em que a variável de ambiente da URL base seleciona um par por `base_url_env`, como `base URL from API_URL` e o par escolhido. Isso inclui a base lida por um campo ou um parcial do Backstage; editar somente o parcial atualiza o motivo e o par na próxima consulta. O `trace` mostra o mesmo motivo.

Em Swift, uma rota Hummingbird (`get`, `post`, `put`, `patch`, `delete` e `on(caminho, method: .get)`) chamada num `Router()` ou num `group(prefixo)` dele vira provedor sob o prefixo de cada `group` em volta, em qualquer profundidade, também quando o roteador passa por argumentos e retornos de funções. O closure da rota é o handler, e `use:` aceita uma função. Os nomes só valem com o import `Hummingbird`; `addRoutes` de um controlador não é lido.

Em C#, uma minimal API vira provedor: `MapGet`, `MapPost`, `MapPut`, `MapPatch`, `MapDelete` e `MapMethods(padrão, métodos, handler)` na aplicação (`WebApplication.Create` ou o `Build()` de `WebApplication.CreateBuilder`) ou num grupo `MapGroup(prefixo)`, com grupos aninhados em qualquer profundidade. O prefixo segue o grupo por variáveis locais, argumentos de método, valores de retorno e métodos de extensão chamados sobre o grupo (`group.MapOrders()`, com o namespace da classe estática importado), então um método de extensão chamado em dois grupos atende sob os dois prefixos. `RequireAuthorization()`, `WithTags(..)` e as outras convenções comuns mantêm o grupo. Um handler lambda vira um handler inline com o nome da rota; um method group (`Handlers.List`) é o método que ele nomeia. Um grupo que o Meada não lê (parâmetro de lambda, como em `UseEndpoints(e => e.MapGet(..))`, um campo, uma variável reatribuída) não gera rota, nem na raiz.

Em C#, um método de uma classe controller (com `[ApiController]` ou derivada de `ControllerBase` ou `Controller`) com `[HttpGet]`, `[HttpPost]`, `[HttpPut]`, `[HttpPatch]`, `[HttpDelete]`, `[HttpHead]` ou `[HttpOptions]` vira provedor, sob o `[Route]` da classe. `[controller]` vira o nome da classe sem o sufixo `Controller`, `[action]` o nome do método, e `{id}`, `{id:int}` e `{id?}` viram parâmetro. Um template que começa com `/` ou `~/` troca o prefixo da classe. Um `[Route]` no método, sem atributo de método, atende todos os métodos (`http::ANY::<caminho>`) e liga a consumidores de qualquer método, com "any method" na razão. Os atributos e as bases valem por `using Microsoft.AspNetCore.Mvc` ou pelo nome completo (`HttpGet` e `HttpGetAttribute` são o mesmo nome); uma classe do repositório com o mesmo nome os esconde. O template precisa ser um literal de string, e um colchete literal se escreve `[[` ou `]]`; qualquer outro `[token]` não gera rota. `[action]` perde o sufixo `Async` (como no ASP.NET Core por padrão) ou vale o `[ActionName("x")]` literal. Sem `[Route]` na classe e sem template no método, a rota é convencional (`MapControllerRoute`), que não é lida; `[Route("")]` é a raiz. Só métodos públicos sem `[NonAction]` valem. Um `[Route]` que não é do MVC deixa a classe ou o método sem rota. Não geram rota: um controller que herda de outra classe do repositório, uma classe que é base de outra, e uma classe declarada em partes (`partial`) em vários arquivos.

Em C#, uma chamada de `GetAsync`, `PostAsync`, `PutAsync`, `PatchAsync`, `DeleteAsync`, `GetStringAsync`, `GetStreamAsync`, `GetByteArrayAsync`, `GetFromJsonAsync`, `PostAsJsonAsync`, `PutAsJsonAsync` ou `SendAsync(new HttpRequestMessage(HttpMethod.X, url))` em um `HttpClient` é um consumer. O receptor só vale quando é um `HttpClient` pelo tipo declarado (campo, parâmetro ou local tipado) ou pela construção, resolvido pelos usings, nunca só pelo nome. A URL dobra literais, interpolação (`$"{base}/users/{id}"`), strings verbatim e raw, `+`, e campos `const` e `static readonly`, também de outras classes; uma parte que não pode ser avaliada vira parâmetro. Um local ligado uma vez (`var url = $"..."`) é seguido; uma ligação atribuída de novo não tem um valor só no seu próprio escopo; o mesmo nome em outro método continua independente. `BaseAddress = new Uri(base)` no cliente, no inicializador ou por atribuição (uma só), é o host das suas URLs relativas, juntadas como `Uri` faz (`https://c/v1` e `orders` dão `/orders`), e um método que recebe o caminho (ou o método, ou o cliente) é lido em cada um dos seus chamadores. Uma URL que não pode ser avaliada é um consumer não resolvido. Clientes de `IHttpClientFactory.CreateClient(name)` não são lidos.

Em C#, um typed client registrado com `AddHttpClient<T>(c => c.BaseAddress = new Uri(base))` (também `AddHttpClient<I, T>`) dá essa base ao `HttpClient` que o contêiner passa ao construtor de `T` e a toda chamada nele dentro de `T`, também por um campo `readonly` que o construtor atribui. A base é avaliada como uma URL e pode vir de uma leitura do ambiente ou da configuração; um `BaseAddress` que o construtor define no seu parâmetro prevalece. Dois registros com bases diferentes, uma base que não se avalia, um registro que o Meada não lê (um method group, uma lambda que repassa o cliente, um `ConfigureHttpClient` encadeado), um `BaseAddress` que o construtor define de forma condicional ou diferente do registro, ou um construtor que o próprio código chama (`new T(..)`, um `T t = new(..)` tipado) deixam a base desconhecida. O parâmetro de um construtor primário (`class T(HttpClient http)`) não recebe injeção. Cada método de uma interface Refit com `[Get("/x")]`, `[Post]`, `[Put]`, `[Patch]`, `[Delete]` ou `[Head]` (por `using Refit`) é um consumidor, com a base de cada `RestService.For<T>(url)` cujo `T` é a interface ou uma interface que a estende, o caminho anexado ao caminho da própria base; quem o chama o alcança por um receptor de tipo declarado (um campo, um parâmetro, uma variável local tipada), então o `trace` vai de quem chama, pelo método da interface, até o provedor; `var api = RestService.For<T>(..)` não dá tipo a `api`. Uma interface que nenhum `RestService.For` cria, como uma registrada com `AddRefitClient`, tem base desconhecida.

Em Swift, uma chamada `data(from:)` ou `data(for:)` numa `URLSession`, `AF.request(url, method:)` (ou num `Session`) e `execute` de um `HTTPClientRequest(url:)` com `method` são consumidores; o método é GET quando não escrito, e o `httpMethod` atribuído a um `URLRequest` vira o método. A URL dobra `URL(string:)`, `URL(string:relativeTo:)`, `appendingPathComponent`, literais, interpolação, `+` e constantes `let` (`static let` também, de outros arquivos e módulos). Uma função que repassa a URL, o método ou a requisição como parâmetro é um wrapper, em qualquer profundidade; uma URL ilegível é um consumidor não resolvido. `dataTask(with:)` não é lido.

Em Kotlin, uma chamada do cliente Ktor (`get`, `post`, `put`, `patch`, `delete`, `head`, `options` e `request(url) { method = HttpMethod.X }`, sem método vale `GET`) num `HttpClient` é consumidor. Um `defaultRequest { url(base) }` na configuração do cliente é a base das URLs relativas. A URL dobra literais, templates (`"$base/users/${id}"`), `+`, `const val` e propriedades `val` iniciadas com uma expressão que dobra, também de outros arquivos; uma parte que não dobra vira parâmetro. Uma função que embrulha o cliente vira consumidor em cada chamada, com os argumentos substituídos, em qualquer número de níveis; uma chamada cuja URL não dá para avaliar é consumidor sem resolução, e `url { path(..) }` não é lido. O método que o bloco de `request` atribui (`method = m` ou `this.method = m`) vem do argumento do embrulho em cada chamada; uma atribuição que pode não rodar (um `if`, uma lambda dentro do bloco), um bloco que não é uma lambda escrita na chamada ou um método que não é um membro de `HttpMethod` deixa o método desconhecido, nunca `GET`. O cliente pode ser importado por `*`, escrito pelo tipo, ou guardado num `val` local sem tipo (`val c = HttpClient(CIO)`, `HttpClient(CIO).use { it.get(..) }`); um `var` não é lido, e um local com o nome de um client do arquivo o esconde, com a própria base. Os nomes valem só com o import do Ktor e um receptor que seja um `HttpClient`.

Em Kotlin, `RestTemplate`, `WebClient`, OkHttp e Retrofit seguem as regras de Java, escritas à moda do Kotlin: `RestTemplate()` e `Request.Builder()` sem `new`, argumentos nomeados, `Api::class.java` como serviço Retrofit e uma interface Retrofit de `fun`s anotadas (as anotações podem vir de `retrofit2.http.*`). O cliente pode estar em uma propriedade, em uma propriedade do construtor ou em um `val` local (`val x = RestTemplate()`, `val web = WebClient.create(..)`), e sua base é a da chamada que o criou. A extensão `retrofit.create<Api>()` (com `import retrofit2.create`) cria a interface como `create(Api::class.java)`. Um argumento nomeado em um passo aninhado do builder e um builder do OkHttp em um local não são lidos, e os nomes só contam com o import da biblioteca.

Os consumidores Java são os métodos de `RestTemplate` (`getForObject`, `getForEntity`, `postForObject`, `postForEntity`, `put`, `patchForObject`, `delete` e `exchange(url, HttpMethod.X, ...)`), o `.uri(...)` de um pedido WebClient (`.get()`, `.post()` e os demais, ou `.method(HttpMethod.X)`), e o `build()` de uma cadeia `HttpRequest.newBuilder()` (`.uri(URI.create(url))`, `.GET()`, `.POST(...)`, `.PUT(...)`, `.DELETE()`, `.HEAD()` ou `.method("X", ...)`) ou `new Request.Builder()` do OkHttp (`.url(url)`, `.get()`, `.post(...)` e os demais ou `.method("X", ...)`); sem método na cadeia, vale `GET`. O cliente vale pelo tipo que o código escreve (campo, parâmetro, variável), importado por nome ou por um `import ...*` que seja o único a poder trazê-lo, ou pela chamada que o cria, e a base de um WebClient vem do `WebClient.create(base)` ou `WebClient.builder().baseUrl(base)` de onde ele sai, também por uma constante, por um campo `final` com um único valor ou pelo argumento que cada chamador passa a um wrapper; de um WebClient guardado numa variável local só o tipo é lido, e a base fica desconhecida. A URL dobra literais, `+`, constantes `static final` (também de outras classes), `URI.create(...)`, `String.format` (cada `%s` ou `%d` recebe o argumento correspondente, e vira parâmetro quando ele não se lê) e variáveis de template (`{id}`). Um método que monta a URL ou o método a partir dos seus parâmetros é um wrapper: cada chamada vira um consumidor com os argumentos substituídos, em qualquer número de níveis. `HttpMethod.X` vale por import do Spring, lido no arquivo que o escreve, inclusive quando chega por um wrapper; um tipo do repositório com o mesmo nome o esconde. Uma chamada cuja URL não se lê é um consumidor não resolvido.

Em Java, uma interface `@FeignClient(name = "x", url = "...")` (`org.springframework.cloud.openfeign`) com métodos anotados como no Spring MVC (`@GetMapping`, `@RequestMapping(method = ...)` e os demais) tem cada método como consumidor, no próprio método da interface: quem o chama chega ao provedor, e `trace` e `context` passam por ele. A base vem de `url` e o prefixo de `path`, ambos com placeholders `${chave}` resolvidos pela configuração do Spring, como no `@Value`. Uma `url` sem esquema recebe `http://`, como no Spring Cloud. Um `@RequestMapping` sem `method` vale `GET`, como o Feign o envia. Sem `url` o host fica sem resolver, e a razão do vínculo traz o `name` (`host unresolved (Feign client x)`); vale o provedor que atende o caminho e o método. `@RequestLine`, outros contratos do Feign e `@RequestMapping` na interface não são lidos. Em Retrofit, uma interface cujos métodos levam `@GET("x")`, `@POST` e os demais de `retrofit2.http` tem cada método como consumidor; a base vem do `Retrofit.Builder().baseUrl(base)` que cria a interface (`retrofit.create(Api.class)`), também por constante ou wrapper, um consumidor por criação, e sem criação a base fica desconhecida. Um caminho que começa com `/` troca o caminho da base (também quando ela é concatenada), e um relativo o acompanha; um caminho vazio é o `@Url` dinâmico do Retrofit e dá um consumidor sem contrato. Uma anotação do Retrofit tem o caminho só em `value`, e um caminho com `{` que não fecha no próprio segmento não é lido. `@HTTP` não é lido. As anotações valem por import, com as mesmas regras de esconder do Spring. Um método Feign ou Retrofit ambíguo (por exemplo, sem `url` e com dois repositórios que servem o caminho) ou não resolvido é um item aberto no próprio método: `search` com `kind=ambiguous` ou `kind=unresolved` o lista, e `meada decide` o resolve com o método como trecho.

Em Rust, `base_url_env` pode ligar uma URL base lida por `std::env::var`, `env!` ou `dotenvy::var`. Em Go, cobre `os.Getenv` e `os.LookupEnv`, inclusive valores padrão com `if v == ""`, `len(v) == 0`, `!ok` ou `!ok || v == ""` quando `ok` vem do mesmo `os.LookupEnv`. Também acompanha `strings.TrimSuffix`, `TrimRight` e `TrimSpace`, e um campo de struct com a tag `env:"X"` ou `envconfig:"X"` (cleanenv, caarlos0/env, envconfig) do mesmo arquivo. Um campo de struct que guarda a URL base (`c.base`) é a leitura que todo composite literal do repositório que o preenche lhe dá, direto (`&Client{base: os.Getenv("X")}`) ou pelo parâmetro de um construtor (`New(base string)`) cujos chamadores no repositório passam todos a mesma leitura; vale num método que lê o campo direto ou por um wrapper de requisição. O campo fica desconhecido quando dois literais ou dois chamadores dão leituras diferentes, quando um literal o preenche por posição ou com um tipo que o código não diz (um alias), quando nenhum literal o preenche, quando algum arquivo o escreve por um seletor (`c.base = x`), quando o construtor não tem chamador no repositório e quando um valor é uma string literal em vez de uma leitura.

Em Java, `base_url_env` liga a URL base de `System.getenv("X")`, também quando se segue uma variável local que guarda a leitura (uma que o arquivo nunca atribui de novo), e a de um campo ou parâmetro de construtor com `@Value("${chave}")` do Spring. A chave vem do `application.properties`, do `application.yml` ou do `application.yaml` em `src/main/resources` do módulo do arquivo (o da raiz de código principal que o build Maven ou Gradle declara para ele, um `srcDirs` do Gradle incluído, ou senão o do `src/main/` em que ele está); com mais de um, vale o `.properties`, depois o `.yml`. Num `.properties`, o que vem depois de `#---` não é lido. Uma URL literal liga pelo host aos `[serve] hosts` dos pares. Um placeholder `${VAR}` que nenhuma chave preenche é a variável `VAR`, mapeada por `base_url_env`. Com padrão (`${VAR:https://web.example.com/api}`), o padrão liga pelo host quando algum par serve esse host; senão vale `VAR`, como sem padrão. O Meada nunca lê o valor de uma variável. Um campo `static`, um campo que algum arquivo atribui, ou um que um parâmetro ou uma variável local escondem, não recebe valor. Um parâmetro de construtor só recebe o valor do Spring quando nenhuma chamada no código cria a classe, `this(..)`, `super(..)` e um `new` dentro de classe anônima incluídos; com chamadas, o valor do chamador prevalece. Perfis (`application-<perfil>`), binding relaxado de variáveis e SpEL (`#{...}`) não são lidos.

Em Kotlin, `base_url_env` liga a URL base de `System.getenv("X")` e de `System.getenv()["X"]`, também por fallbacks `?:`, `!!`, parênteses e métodos de string como `trim`, `trimEnd`, `removeSuffix` ou `lowercase`, e a de uma propriedade ou propriedade de construtor com `@Value("\${key}")` do Spring (o `$` escapado em string Kotlin), lida pelo cliente Ktor e pelos Retrofit, RestTemplate e WebClient em Kotlin. A configuração é a do Java (`application.properties` ou `application.yml` no `src/main/resources` do módulo), também em um repositório sem arquivo Java, com as mesmas regras: uma URL literal liga por host, um placeholder `${VAR}` é a variável `VAR`, um default cujo host um peer serve liga por host, e perfis, relaxed binding e SpEL não são lidos. A leitura pode estar em uma propriedade, em um `val` local da função ou na própria expressão da URL; um `var` não é seguido. O Meada nunca lê o valor de uma variável.

Em C#, `base_url_env` liga a URL base de `Environment.GetEnvironmentVariable("X")` (também `System.Environment`), por fallbacks `??`, `!` e `Trim`, `TrimStart` ou `TrimEnd`, e a de `configuration["Secao:Chave"]` num receptor declarado `IConfiguration` ou em `builder.Configuration` de um `WebApplicationBuilder`, lida pelas chamadas de `HttpClient` e pelo `BaseAddress` de um cliente. Uma chave de configuração é a variável `Secao__Chave`, como o provedor de ambiente do .NET a nomeia e como o código escreve a chave (`base_url_env` compara como escrito, embora o .NET ignore maiúsculas). A leitura pode estar num local ou num campo. Uma cadeia `??` conta quando exatamente um dos seus operandos é uma leitura. A chave é buscada no `appsettings.json` do projeto (comentários, vírgulas finais e a marca de ordem de bytes são aceitos, objetos aninhados se juntam com `:` e as chaves se comparam sem diferenciar maiúsculas); uma URL literal que ele guarda é o padrão dessa variável: quando o `[serve] hosts` de um peer nomeia o host, a chamada liga por host, e senão por `base_url_env`. `appsettings.<Ambiente>.json`, `GetValue`, `GetSection`, binding de opções e user secrets não são lidos, e uma leitura passada por um wrapper de outro arquivo é um parâmetro. O Meada nunca lê o valor de uma variável.

Em Swift, `base_url_env` liga a URL base de `ProcessInfo.processInfo.environment["X"]` (também `Foundation.ProcessInfo`), por `!`, parênteses, fallbacks `??` e métodos de string como `trimmingCharacters`, `replacingOccurrences`, `lowercased` ou `uppercased`. Um `??` conta quando o fallback é um literal de string simples; uma segunda leitura no fallback ou num argumento, um fallback que é uma chamada ou um nome, e uma chave que não é literal deixam a base desconhecida. A leitura pode estar guardada num `let` (local, `static let` ou de nível superior), num `if let` ou `guard let` só dela ou aparecer na própria URL, também dentro de uma interpolação. Um `var` atribuído de novo, um `if var` ou `guard var` e um optional binding de qualquer coisa além de uma leitura não são seguidos, e o `Environment.get("X")` do Vapor não é uma leitura. O Meada nunca lê o valor de uma variável.

Os recursos declarados em `meada.toml` também geram contratos. Com a tabela `[serve]`, o repositório lê OpenAPI 3.0/3.1 e AsyncAPI 2.x/3.0, encontrados pelo nome ou listados em `[serve] documents`. Rotas e canais viram contratos com campos de resposta ou payload. `meada status` mostra o que foi lido e ignorado.

## Canais de mensagens

Cada produtor se liga ao handler dos consumidores, inclusive no mesmo repositório. Swift ainda não tem suporte a canais na v1.

| Canal | TypeScript / JavaScript | Python | Rust | Go | Java | Kotlin | C# |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Kafka | `kafkajs` | `confluent-kafka`, `kafka-python` | `rdkafka` | `segmentio/kafka-go`, `confluent-kafka-go`, `sarama` | Spring Kafka | Spring Kafka | `Confluent.Kafka` |
| RabbitMQ | `amqplib` | `pika` | `lapin` | `amqp091-go` | Spring AMQP | Spring AMQP | `RabbitMQ.Client` |
| Amazon SQS | AWS SDK v3, `sqs-consumer` | `boto3` | `aws-sdk-sqs` | `aws-sdk-go-v2` | AWS SDK v2 | AWS SDK v2 | `AWSSDK.SQS` |
| Amazon SNS | AWS SDK v3 `PublishCommand`, `PublishBatchCommand` | `boto3` `publish`, `publish_batch` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon EventBridge | AWS SDK v3 `PutEventsCommand` | `boto3` `put_events` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| Amazon Kinesis | AWS SDK v3 `PutRecordCommand`, `PutRecordsCommand` | `boto3` `put_record`, `put_records` | | AWS SDK v2 | AWS SDK 2.x | | AWS SDK .NET |
| NATS | `nats` | `nats-py` | `async-nats` | `nats.go` | jnats | jnats | `NATS.Client` |
| Redis streams | `ioredis`, `node-redis` | `redis-py` | crate `redis` | `go-redis` | Jedis, Lettuce | Jedis, Lettuce | `StackExchange.Redis` |
| JMS | | | | | Spring JMS | Spring JMS | |
| Azure Service Bus | Azure SDK | Azure SDK | | Azure SDK | Azure SDK | | `Azure.Messaging.ServiceBus` |

Os brokers são os ids dos contratos (`channel::<broker>::<nome>`): `kafka`, `amqp` (RabbitMQ), `jms`, `sqs`, `nats`, `redis`, `bullmq` e `servicebus` (Azure Service Bus). Também há suporte a jobs BullMQ. No RabbitMQ, um `publish` na exchange chega às filas ligadas pelos bindings. NATS considera os curingas `*` e `>`. Clusters de broker não são modelados: tópicos com o mesmo nome em clusters diferentes podem se ligar como se pertencessem a um só.

Em Java, fora das formas AWS, o cliente vale pelo tipo que o código escreve para ele (`KafkaTemplate`, `RabbitTemplate`, `JmsTemplate`, `Connection` ou `Dispatcher` do jnats, `Jedis`, `JedisPooled`, `UnifiedJedis`, `JedisCluster` e os `RedisCommands`, `RedisAsyncCommands` e `RedisReactiveCommands` do Lettuce), e uma anotação de listener pelo import do Spring; um tipo do repositório com o mesmo nome (classe, interface ou anotação `@interface`) o esconde. Produzem `KafkaTemplate.send(tópico, ...)`, `RabbitTemplate.convertAndSend(exchange, routingKey, ...)` (com dois argumentos, ou com um `MessagePostProcessor`, publica na exchange padrão e chega à fila da chave, ou na `spring.rabbitmq.template.exchange` da configuração), `JmsTemplate.send` e `convertAndSend` com o destino, `sendMessage` com o `queueUrl` do pedido (a fila é o último segmento da URL), `publish(assunto, ...)` e `xadd(chave, ...)`. Consomem `@KafkaListener(topics = ...)`, também por `topicPartitions`, `@RabbitListener(queues = ...)`, `queuesToDeclare` e `bindings = @QueueBinding(...)`, cujas chaves viram bindings (numa `@Exchange` `direct`, o padrão, ou `fanout`, só as chaves sem curinga; numa `headers`, nenhuma), `@JmsListener(destination = ...)`, `receiveMessage`, `subscribe(assunto)` e as streams de `xreadGroup` (Jedis, de um `Map.of`) e `xreadgroup` (Lettuce, de `StreamOffset`). `BindingBuilder.bind(new Queue(..)).to(new TopicExchange(..)).with(chave)` também liga uma fila. O handler de uma anotação é o método anotado; o de um cliente que faz polling é o método com a chamada que nomeia o tópico ou a fila, mesmo que o laço de recepção fique em outro. Os nomes se avaliam como as URLs: constantes, campos `final`, os argumentos de um wrapper, um campo com `@Value("${chave}")` e os placeholders `${chave}` das anotações, lidos da configuração Spring do módulo. Um nome que não se avalia (uma variável de ambiente, SpEL, uma sobrecarga que os argumentos não distinguem) é um lado não resolvido.

Em Kotlin, os mesmos clientes, anotações e bindings valem escritos à moda do Kotlin, com as regras de Java para handlers e nomes: argumentos nomeados e arrays (`@KafkaListener(topics = ["a", "b"])`, `arrayOf(..)`), anotações aninhadas sem `@` (`QueueBinding(value = Queue("q"), exchange = Exchange(value = "x"), key = ["k"])`), `Queue("q")` sem `new` em `BindingBuilder`, `const val` de topo ou de um `companion object`, templates (`"orders-$env"` é um lado não resolvido enquanto `env` não se avalia), um cliente recebido pelo construtor (`private val kafka: KafkaTemplate<String, String>`), `@Value("\${chave}")` e `mapOf(chave to id)` nas streams do Jedis e do Lettuce. Uma chamada num receptor implícito (função de extensão do cliente, blocos `apply` e `with`) não é lida.

Em C#, fora das formas AWS, o cliente vale pelo tipo que o código escreve para ele (campo, propriedade, parâmetro ou local tipado) ou pela construção que o criou (`new ProducerBuilder<..>(..).Build()`, `factory.CreateConnection()`, `connection.CreateModel()`, `multiplexer.GetDatabase()`, `client.CreateSender(..)`), resolvido pelos usings, nunca só pelo nome: um tipo do repositório com o mesmo nome o esconde, e `ConnectionFactory` ou `IConnection` com `RabbitMQ.Client` e `NATS.Client` importados juntos não é nenhum dos dois. Produzem `Produce` e `ProduceAsync(tópico, ...)` do Confluent.Kafka, `BasicPublish` e `BasicPublishAsync(exchange, routingKey, ...)` (a exchange vazia, `string.Empty`, `String.Empty` com `using System;` ou `""`, chega à fila da chave), `SendMessageAsync` do AWSSDK.SQS com a URL da fila ou com `new SendMessageRequest { QueueUrl = .. }` (a fila é o último segmento da URL), `Publish(assunto, ...)` do NATS.Client, `StreamAdd` e `StreamAddAsync(chave, ...)` do StackExchange.Redis e os métodos de envio de um `ServiceBusSender` (`SendMessageAsync`, `SendMessagesAsync`, `ScheduleMessageAsync`, `ScheduleMessagesAsync`), cuja fila ou tópico é o argumento do `CreateSender` que o criou. Consomem `Subscribe(tópico)` e `Subscribe(tópicos)`, `BasicConsume(fila, ...)`, `ReceiveMessageAsync`, `SubscribeAsync(assunto, handler)`, `StreamReadGroup` e `StreamReadGroupAsync(chave, ...)` e `CreateProcessor(fila)` ou `CreateProcessor(tópico, assinatura)`. `QueueBind(fila, exchange, chave)` liga uma fila pelas regras de roteamento do RabbitMQ, e um vínculo do NATS por curinga fica `inferred`. No Azure Service Bus não há curinga, e um processador num tópico consome toda mensagem enviada ao tópico, seja qual for a assinatura. O handler de um callback é o próprio callback (uma lambda ou um método do repositório passado ao `SubscribeAsync`) ou cada handler atribuído com `+=` ao evento `Received`, `ReceivedAsync`, `MessageHandler` ou `ProcessMessageAsync` do consumidor ou do processador que guarda o resultado da chamada (num local, no mesmo método; num campo, em qualquer método da classe), um consumidor por handler; o de um cliente que faz polling é o método com a chamada que nomeia o tópico ou a fila. `.ConfigureAwait(..)` na cadeia que cria o cliente não a interrompe. Os nomes se avaliam como as URLs (constantes, campos `static readonly`, locais ligados uma vez, interpolação e os argumentos de um wrapper em cada chamador, também o do método que cria um `ServiceBusSender` com `CreateSender(fila)`). Um `ServiceBusSender` escrito mais de uma vez (um campo atribuído em dois construtores ou num setter, ou um local refeito) não tem um nome só e fica não resolvido. Um nome que não se avalia, inclusive uma leitura de variável de ambiente ou de `IConfiguration` (o `appsettings.json` não é lido aqui), é um lado não resolvido. MassTransit, `BasicGet`, `SubscribeSync` e `QueueSubscribe`, o NATS.Net v2 não são lidos.

## Pacotes npm

Cada `package.json` com nome gera `package::npm::<nome>`. Dependências em `dependencies`, `devDependencies`, `peerDependencies` e `optionalDependencies` se ligam aos pacotes do workspace, inclusive no próprio repositório. A faixa é comparada à versão local: `range mismatch` quando não aceita a versão, `range unchecked` quando não pode ser conferida.

Chamadas a funções importadas atravessam para o código do provedor por `exports`, `types`, `module`, `main` e re-exports. Se a entrada aponta ao build, o `outDir` do `tsconfig.json` é mapeado de volta ao `rootDir`. `search` com tipo `package` mostra nome, repositório, pasta, versão e consumidores. `context` de uma função exportada lista seus chamadores em todo o workspace.

## Pacotes Python

Cada `pyproject.toml` com nome em `[project]` ou `[tool.poetry]` gera `package::pypi::<nome>`, normalizado pelo PEP 503. Dependências em `[project] dependencies`, `[tool.poetry.dependencies]` e `requirements*.txt` se ligam aos pacotes do workspace. A faixa permanece como escrita e é comparada à versão local como o PEP 440 manda: `range mismatch` quando os especificadores não aceitam a versão, nada a mais quando aceitam, `range unchecked` quando o provedor não tem versão ou um especificador não pode ser lido. São conferidos `==`, `!=`, `<`, `<=`, `>`, `>=`, `~=`, `==X.*` e `===`, separados por vírgula; a sintaxe do Poetry (`^1.2`, `~1.2`, `*`) e os marcadores de ambiente não são conferidos. Uma versão pré-lançamento é rejeitada, a menos que um especificador cite uma. Um mismatch não remove nenhum candidato.

O `[tool.uv.sources]` de um projeto uv escolhe o pacote que uma entrada de `[project] dependencies` instala, como as fontes do Cargo: `{ workspace = true }` o membro do workspace uv do consumidor (o `pyproject.toml` mais próximo, nele ou acima, com `[tool.uv.workspace]` cujos globs de `members`, menos `exclude`, o incluem), `{ path = "..." }` o pacote desse diretório e `{ git = "..." }` o repositório do workspace dessa URL, cada um com nível `exact`, para a entrada e para as chamadas pelos seus imports. Os outros pacotes do mesmo nome não recebem ligação. Uma fonte `url` ou `index` fica de fora. Uma lista de fontes (uma por marker), e uma fonte workspace cuja raiz ou cujo único membro com o nome não se encontra, seguem a regra acima. Um pacote do próprio repositório do consumidor que nenhuma fonte uv nomeia (uma entrada de `[project] dependencies` ou de `requirements*.txt` sem fonte) só é `inferred` quando é o único pacote do nome, com o motivo "no uv source names the local package, the registry package is the likely provider": pip e uv instalam o pacote do registro, então um stub homônimo no repositório é um candidato, não uma certeza. Um único provedor em outro repositório do workspace segue `exact`.

Um import chega ao código do provedor com nível `exact` quando o módulo tem o nome da distribuição, `inferred` quando é um módulo de topo dela e `ambiguous` quando várias distribuições o expõem. Pacotes npm e PyPI com o mesmo nome não se ligam.

## Pacotes Rust

Cada `Cargo.toml` com `[package] name` gera `package::cargo::<nome>`. Cada membro de um workspace Cargo é um provedor. Entradas em `[dependencies]`, `[dev-dependencies]` e `[build-dependencies]` são resolvidas assim:

- `path`: crate da pasta, no mesmo repositório ou em outro do workspace. O caminho é resolvido pelo texto a partir das raízes, sem ler fora delas nem seguir links simbólicos. Um caminho fora do workspace não aparece.
- `git`: repositório identificado pela URL normalizada.
- Versão: provedor do mesmo nome no workspace.

`package =` informa o nome real de uma dependência renomeada. `workspace = true` lê `[workspace.dependencies]`; `version.workspace = true` lê `[workspace.package]`.

A ligação é `exact`, com faixa como escrita e `range unchecked`, pois faixas Cargo não são conferidas. Um `use` segue o `[lib] name` ou a chave da dependência renomeada e atravessa os `pub use` até os itens públicos. Essa ligação também é `exact`, com `; import crate name` na razão. Dependências fora do workspace não aparecem.

## Módulos Go

Cada `go.mod` com `module` gera `package::go::<caminho do módulo>`. `require` e `replace` para pastas locais se ligam ao provedor no workspace. Um `replace` que sai do repositório é resolvido pelo texto, como `path` no Cargo; um `replace` por outro módulo (`replace a => github.com/owner/repo`) mantém, entre os provedores do módulo requerido, o do repositório do workspace cuja identidade é o início desse caminho (um sufixo `/vN` é descartado; um subdiretório indica a pasta do `go.mod`), em `exact`, para a entrada e para as chamadas pelos seus imports. Um replace que nenhum repositório do workspace contém, ou cujo `go.mod` declara outro caminho de módulo, fica de fora.

A ligação é `exact`, com a versão do `require` como faixa, `(local none)` como versão local e `range unchecked`. Um import de `<módulo>/<pasta>` chega às funções, tipos e métodos exportados da pasta. Quando ela existe, a ligação é `exact`, com `; import module path` na razão.

O Meada não lê `go.sum` nem lockfiles, incluindo `Cargo.lock`. Não consulta o proxy de módulos, o registro npm, PyPI ou crates.io.

## Pacotes Maven e Gradle

Cada `pom.xml` com coordenadas literais e cada projeto Gradle com `group` e nome literais gera `package::maven::<grupo>:<artefato>`. O Maven herda o grupo literal de `<parent>` quando necessário; cada módulo é um artefato. No Gradle, `rootProject.name` e `include` de `settings.gradle` ou `settings.gradle.kts` nomeiam os projetos dos arquivos `build.gradle` ou `build.gradle.kts`. Um projeto renomeado ou movido em `settings.gradle` (`project(":app").name`, `.projectDir`) e um `group` definido em `allprojects` não geram artefato.

Dependências Maven e Gradle, inclusive na notação de mapa (`group: 'g', name: 'a'`), se ligam aos artefatos do workspace com nível `exact` e `range unchecked`. Uma versão calculada (`${project.version}`, `$v`) mantém a ligação `exact`, com a versão como foi escrita. Um grupo ou artefato que usa propriedade ou variável se resolve quando os arquivos de build declaram o valor como literal (uma entrada de `<properties>` do pom, `project.groupId` e `project.artifactId`, um pom pai do repositório, um `val`, `def` ou `ext` do Gradle atribuído uma só vez a uma string simples) e se liga como um escrito; do contrário fica `ambiguous`, mesmo com um único candidato, e as partes literais filtram os candidatos, e uma coordenada sem nenhuma parte literal não gera dependência. Uma declaração Gradle sem coordenada, como `project(":tools")`, `files(...)`, `platform(...)` ou uma chamada, não gera dependência. O `gradle.properties` do repositório, o da raiz do `settings.gradle` e o do módulo (o do módulo vence) também preenchem um `${nome}` ou `$nome` da coordenada e da versão, desde que o script não atribua o nome; e um acessor de catálogo de versões (`libs.models`, `libs.meu.lib` para `meu-lib` ou `meu_lib`) vira a coordenada que a tabela `[libraries]` de `gradle/libs.versions.toml` (ou do arquivo `*.versions.toml` que um `from(files("..."))` do `settings.gradle` nomeia por um caminho relativo) dá a ele, com a versão de `[versions]`. Um alias que o catálogo não tem não gera dependência. `buildSrc`, plugins de convenção, builds incluídos e BOMs resolvidos na compilação não são lidos. Os dois arquivos são lidos com limite de tamanho, numa passada. Dependências externas ficam de fora. Os leitores são compartilhados com Kotlin, sem executar JDK, Maven ou Gradle e sem consultar registros.

Um `import` de uma classe de um artefato do workspace de que o repositório depende chega à classe no provedor, pela declaração `package` dos arquivos e não por um caminho: `context` num método público lista as chamadas de outros repositórios que chegam a ele, e numa classe pública as que chegam aos seus construtores e métodos, uma linha por chamada, e `trace` atravessa o salto. Vale para chamadas `Tipo.metodo()`, `new Tipo(...)`, chamadas sobre uma variável, campo ou parâmetro do tipo importado, importações `import a.b.*` e importações estáticas. O método é escolhido pelo nome e pelo número de argumentos entre as sobrecargas da classe e das superclasses e interfaces que ela tem no provedor, como o Java as junta. A ligação é `exact` quando um único provedor declara o pacote e o tipo, `ambiguous` quando vários declaram ou quando sobrecargas com o mesmo número de argumentos restam, e `inferred` quando um ancestral da classe vem de fora do repositório do provedor, porque ele pode declarar uma sobrecarga melhor; o motivo termina em `; import package`. Duas importações `import a.*` que trazem uma classe com o mesmo nome não geram ligação. As raízes de código são `src/main/java` e `src/test/java` de cada módulo Maven e os diretórios literais de `sourceSets { ... java { srcDirs = [...] } }` do Gradle (`srcDir`, `srcDirs +=` e a forma Kotlin DSL também). Classes de uma raiz de teste, ou de um conjunto que não seja `main`, nunca são importadas por um dependente; um arquivo fora de qualquer raiz ainda resolve pela declaração `package`. Diretórios calculados, ou dentro de `allprojects`, `subprojects`, `project(':x')`, `configure(...)` ou de um bloco condicional, não são lidos. Um módulo do mesmo repositório de que o arquivo depende passa adiante as próprias dependências `compile` que não são `optional`, como o Maven faz; as de escopo `test`, `provided`, `runtime` ou `system` ficam com ele, e uma dependência do Gradle não é seguida.

Em Kotlin, um `import` de função de topo, de função de extensão ou de classe desse artefato chega à declaração do mesmo jeito: `greet("x")` escolhe a função do pacote importado pelo nome e pela quantidade de argumentos, `Money(5)` chega à classe, `Money.of(5)` ao `companion` sem nome e `order.format()` a uma função de extensão da classe importada. Uma função de extensão só é seguida pelo `import` do seu nome (ou de um `import` com `*` do seu pacote) e fica `exact` quando o tipo escrito do receptor é uma classe do artefato, cujos membros são todos conhecidos, e uma só extensão desse tipo cabe na chamada; num receptor de tipo desconhecido ou de fora do workspace, fica `inferred`, porque um membro desse tipo pode escondê-la. Um membro de mesmo nome no tipo do receptor prevalece sobre ela, e uma que o próprio repositório declara não é confundida com ela. Uma chamada que pode deixar um argumento com valor padrão de fora, ou que preenche um `vararg`, chega a todas as sobrecargas que poderia, `ambiguous` quando há mais de uma; uma declaração `internal` ou `private`, e uma sob raiz de teste não são seguidas. As raízes de fonte incluem também `src/main/kotlin` e `src/test/kotlin`, e os diretórios de `sourceSets` do Gradle são lidos para `kotlin { srcDirs = [...] }`, `srcDir("...")`, `sourceSets["main"]` e `sourceSets.getByName("test")` como para `java`; um conjunto Kotlin é de teste quando se chama `test` ou termina em `Test`, e de produção quando se chama `main` ou termina em `Main`, e qualquer outro não é lido. Um `companion` com nome (`companion object Factory`) e `setSrcDirs` não são seguidos.

## Pacotes NuGet

Cada `.csproj` com SDK provê `package::nuget::<id>`, onde o id é o `PackageId`, senão o `AssemblyName`, senão o nome do arquivo do projeto. O `PackageId`, o `AssemblyName` e a versão escritos num `Directory.Build.props` valem para os projetos abaixo dele, e os do próprio `.csproj` vencem. Um id computado (`$(Id)`), no projeto ou no `props`, não provê pacote. Um `PackageReference` que nomeia um pacote do workspace liga a ele em `exact`, com o atributo (ou elemento) `Version` como faixa e `range unchecked`. Um `ProjectReference` para um arquivo de projeto de outro repositório do workspace liga ao pacote desse projeto em `exact`, qualquer que seja o que o nome do arquivo diga do id; o que aponta para um projeto do próprio repositório fica dentro dele. Uma dependência fora do workspace fica de fora, e o Meada não consulta feed nem lê `Directory.Build.targets` nem `Directory.Packages.props`. Os ids são comparados sem diferenciar maiúsculas de minúsculas, como o NuGet faz; o caminho de um `ProjectReference` é comparado na caixa escrita.

Um `using` de um namespace de um projeto de que o arquivo depende alcança seus tipos por namespace: `context` em um método ou tipo público lista as chamadas de outros repositórios que o alcançam, e `trace` cruza o salto. Cobre `Tipo.Metodo()`, `new Tipo(...)`, chamadas em variável, campo, propriedade ou parâmetro do tipo, mesmo que nenhuma chamada do arquivo cite o nome do tipo, usings de namespace e `using static`. O método é escolhido por nome e quantidade de argumentos. O link é `exact` quando um provedor declara o namespace e o tipo, `ambiguous` quando vários declaram e `inferred` quando um ancestral da classe vem de fora do repositório do provedor; a razão termina com `; import namespace`. Métodos de extensão, um local `var` que guarda o resultado de uma chamada e um nome totalmente qualificado escrito sem `using` não são seguidos. O arquivo também vê o que os projetos do próprio repositório referenciados pelo seu projeto, direta ou indiretamente, passam adiante; num nome repetido, vence a referência do projeto mais próximo. Uma referência com `PrivateAssets` que inclui `all` ou `compile`, ou computado (`$(...)`), não passa adiante.

## Pacotes Swift

Cada `Package.swift` provê `package::swiftpm::<nome>`, onde o nome é o literal de `Package(name:)`; um nome calculado provê o pacote nomeado pelo seu padrão, com `*` no lugar de cada parte calculada (`Kit*`). Os produtos `.library(name:targets:)` e os alvos de cada um ficam registrados, e só os módulos de um produto de biblioteca podem ser importados. Um `.package(url:)` liga ao repositório do workspace cuja identidade de remoto (ADR 0004) é a da URL, e um `.package(path:)` ao repositório nesse caminho, em `exact`, com a faixa escrita (`from`, `exact`, `branch`, `revision`, `.upToNextMajor`, intervalos) como faixa, `(local none)` e `range unchecked`. Só valores literais contam: uma URL, um caminho ou um nome calculado (`"\(vendor)"`) torna a ligação `ambiguous`, listando todo provedor que cabe nas partes literais, e o mesmo vale para um pacote cujo `Package(name:)` é calculado. Uma dependência fora do workspace fica de fora, e o Meada não lê `Package.resolved` nem consulta a rede.

Um `import Modulo` que nomeia um módulo de um pacote de que o `Package.swift` depende alcança suas declarações exportadas: `Modulo.f(a:)`, `f(a:)`, `Tipo(a:)` e `valor.metodo(b:)` de uma variável, parâmetro ou constante local de tipo escrito ou construída na hora. O rótulo dos argumentos escolhe entre as sobrecargas, e um parâmetro com valor padrão pode ficar de fora. Só declarações `public` ou `open` são seguidas. O link é `exact` quando um único pacote expõe o módulo e `ambiguous` quando vários pacotes de que o arquivo depende expõem o módulo, mesmo que só um declare o nome; a razão termina com `; import module`. Módulos da plataforma (`Foundation`, `Dispatch`, `SwiftUI` e outros) nunca viram ligação. Todas as dependências do `Package.swift` mais próximo valem para os arquivos abaixo dele, sem separar o que cada alvo declara em `.product(name:package:)`, e um membro `internal` de um tipo público, um membro herdado e uma extensão de outro módulo não são distinguidos.

## Drift de formato

Drift é um campo que um consumidor `fetch` lê da resposta, mas que a rota não devolve. O Meada compara a leitura com OpenAPI, schema zod ou objetos literais do handler.

| Consulta | Resultado |
| --- | --- |
| `trace` | Acrescenta `drift: missing <caminho>` à ligação |
| `context` de uma rota | Mostra origem do formato, consumidores, drift, nível e leituras não conferidas em partes abertas |
| `search` com tipo `drift` | Lista o drift encontrado |
| `meada status` | Conta leituras ausentes no repositório consumidor; o provedor não recebe essa contagem |

## Valores de configuração

Cada escalar de cada arquivo `*.yaml`, `*.yml` e `*.json` que o Meada lê, e dos arquivos YAML e JSON da configuração de CI na raiz (`.github/`, `.gitlab-ci.yml`, `.gitlab/`, `.circleci/`, `.buildkite/`, `.drone.yml`), é um valor de configuração: arquivo, caminho da chave (no estilo do jq, `alerts.loki[0].labels.support`, `metadata.labels["app.kubernetes.io/name"]`, `[2].spec.replicas` num arquivo de vários documentos), valor, tipo (texto, número ou booleano) e linha. Os outros caminhos ocultos ficam de fora, e não há nada para ligar.

Os arquivos Terraform e OpenTofu também são valores de configuração (`*.tf`, `*.tofu`, `*.tf.json`, `*.tofu.json`, `*.tfvars`, `*.auto.tfvars`, `*.tfvars.json`). O caminho da chave passa pelo tipo e pelos rótulos do bloco e pelo nome do atributo (`resource.aws_sqs_queue.orders.name`, `resource.opsgenie_team_routing_rule.critical.criteria.conditions[1].expected_value`), e um bloco repetido com o mesmo tipo e os mesmos rótulos é indexado `[n]` (`ingress[0]`). Um literal mantém o tipo; uma expressão (`var.environment`, `format(..)`) ou um template (`"${var.environment}-orders"`, um heredoc com interpolação) é guardado como foi escrito, com o tipo `expression` ou `template`, até a avaliação estática resolvê-lo. Um arquivo `.tofu` esconde o arquivo `.tf` de mesmo nome na sua pasta (o `.tofu.json`, o `.tf.json`), como o OpenTofu faz, e o arquivo escondido não é lido. Ao lado de um arquivo de override, ele conta como um arquivo que não é lido, porque o Terraform, que ignora os arquivos `.tofu`, o lê.

| Consulta | Resultado |
| --- | --- |
| `search` com kind `value` | Valores cujo valor ou caminho da chave é igual à consulta, começa com ela ou a contém, como `<repositório> <caminho>:<linha> <caminho da chave> = <valor>` |
| `search` sem kind | Valores só quando nenhum símbolo e nenhum contrato casa |
| `meada status` | Arquivos de configuração lidos e ignorados (lockfiles, arquivos que não são lidos) e os valores ocultados |

Um valor acima de 1 KiB é guardado cortado, marcado `(cut)`. Os `data` e `stringData` de um `Secret` do Kubernetes ficam guardados como `redacted`, com caminho da chave, tipo e linha, assim como o `default` de uma `variable` do Terraform com `sensitive = true` (ou `"true"`, que o Terraform converte), também quando outro arquivo da mesma pasta a marca assim e a pasta tem um arquivo de override (`override.tf`, `*_override.tf`), que o Terraform mescla aos outros, e todo valor que um arquivo `.tfvars` dá a uma variável desse nome (todos, quando um arquivo Terraform na mesma pasta ou acima dela não é lido) e o `value` de um recurso `aws_ssm_parameter` com `type = "SecureString"` literal, em HCL ou Terraform JSON; nada mais é filtrado, então os arquivos que não devem entrar no índice vão no `.meadaignore`. Lockfiles (`package-lock.json`, `npm-shrinkwrap.json`, `pnpm-lock.yaml`, `packages.lock.json`), templates Helm (que não são YAML até serem renderizados), arquivos acima de 1 MiB e arquivos de mais de 1.000 documentos ficam de fora. O texto dos valores nunca entra no `meada export`.

Um ARN AWS reconhecido pela tabela de tipos, uma URL SQS ou `s3://archive/caminho` em configuração vira um lado de uso `exact`, localizado por `<arquivo>#<caminho da chave>`. Um nome simples sob a última chave `queue`, `topic`, `bucket` ou `table` vira, respectivamente, um uso SQS, SNS, S3 ou DynamoDB `inferred`. Por exemplo, `queue: orders` em valores Helm usa `channel::sqs::orders`; `other: orders` não liga nada. O uso não tem direção: liga somente aos provisionadores e conta para órfãos, sem virar produtor, consumidor, escritor ou leitor. `context` e a busca de contratos mostram esses usos, e `trace` segue a ligação de provisionamento até a chave e de volta. Valores ocultados, cortados ou não resolvidos ficam de fora; o atributo que define o nome de um provisionador não usa o próprio recurso. O export inclui os lados em `uses`, sem o texto dos valores, e o console os mostra separados dos lados com direção.

## Nós IaC

Cada `resource`, bloco `data` e chamada `module` dos arquivos `*.tf`, `*.tf.json`, `*.tofu` e `*.tofu.json` é um nó IaC, com endereço (`<tipo>.<nome>`, `data.<tipo>.<nome>`, `module.<nome>`), tipo, arquivo, linhas e o módulo raiz que começa a sua avaliação: uma pasta que nenhuma outra chama como módulo local, rotulada pelo caminho (`.` na raiz). Cada chamada de um módulo local (um `source` `./` ou `../` que fica dentro do repositório) instancia os blocos dele com o endereço da chamada na frente (`module.orders.aws_sqs_queue.this`), e um `count` ou um `for_each` sobre valores que se resolvem em literais dá um nó por instância, com `[chave]` no fim (`aws_sqs_queue.dlq[0]`). Uma expressão que nomeia um resource, um bloco data ou um módulo é uma referência, guardada como uma chamada; uma que passa por `var.<nome>`, `local.<nome>` ou a saída de um módulo é seguida até o que ela nomeia, e guarda o nome por onde passou.

O JSON Terraform usa as mesmas regras de avaliação e referência. Strings comuns são templates; uma interpolação sozinha mantém o tipo do resultado, então `${var.items}` pode fornecer uma coleção. Defaults de variáveis e fontes de módulos são literais. Strings estáticas de `depends_on` nomeiam referências. Arrays de blocos mantêm os índices nos caminhos de configuração. As duas extensões JSON aceitam endereços de nós nas localizações `via` de recursos declarados.

| Consulta | Resultado |
| --- | --- |
| `search` com kind `iac` | Nós cujo endereço, tipo ou nome físico (o argumento `name` como a avaliação estática o resolve) é igual à consulta, começa com ela ou a contém, como `<repositório> <caminho>:<linha> <endereço> (<tipo>, name "<nome>", root <módulo raiz>)`, `pattern` no lugar de `name` quando uma parte não se resolve |
| `search` sem kind | Nenhum nó IaC: só os valores de configuração que guardam o nome, quando nada mais casa |
| `context` num endereço | Tipo, nome físico (`(pattern)` quando uma parte não se resolve), módulo raiz, as variáveis da raiz por onde o nome passou (`overridable:`, `(default)` ou `(tfvars)`), as referências nos dois sentidos (`references:`, `referenced by:`, cada uma com o seu `via`) e os valores de configuração do bloco |
| `trace` de um endereço | As referências, `down` (o que o nó alcança) ou `up` (o que o alcança), dentro do módulo raiz e pelos módulos locais |

Tudo o que é referência é `exact`: um nome escrito, seguido sem avaliar valores. A avaliação
estática resolve o nome físico de cada nó e cada `count` e `for_each`, sem rodar o Terraform nem
acessar a rede, a partir dos valores do próprio repositório: os defaults das variáveis, o
`terraform.tfvars`, o `terraform.tfvars.json`, os `*.auto.tfvars` e os `*.auto.tfvars.json` da raiz
(nessa ordem, o arquivo seguinte vencendo), os locals, as entradas e saídas de um módulo local por
chamada, `each`, `count.index`, templates e as funções puras de texto, coleção e conversão do ADR
0025 com `jsonencode`. Arquivos de override (`override.tf`, `*_override.tf`) se mesclam aos blocos
da sua pasta depois dos outros arquivos dela, como o Terraform os aplica: o que um bloco de override
define substitui o do bloco base, referências incluídas, e ao lado de um, um arquivo Terraform
que não é lido deixa sem avaliar todas as variáveis da pasta, porque pode marcar qualquer uma
como sensível. Data sources, atributos de resources,
módulos do registro,
`terraform.workspace`, `-var`, `-var-file`, `TF_VAR_`, variáveis sensíveis e qualquer outra função
nunca são avaliados: essa parte vira um `*` e o nome um padrão (`orders-*`), que liga nomes
literais pelas regras de padrões abaixo. Um nome que passou por uma variável do módulo raiz, pelo default ou por um arquivo de
variáveis, a registra, já que quem faz o deploy pode trocá-la. Um valor acima de 64 KiB, uma
expressão cuja avaliação poderia passar de 1 MiB e o que uma raiz avalia além de 64 MiB ficam sem
resolver. Um arquivo Terraform ou de variáveis alterado avalia de novo só os módulos raiz cuja
unidade (a raiz e os módulos locais que ela chama) o contém. Um `count` ou um `for_each` que não se
resolve dá um nó só, sem chave; um módulo do registro ou remoto é só o nó da chamada. Só os nós com um lado de contrato entram no `meada
export`.

### Contratos órfãos

`search` sem filtro, com `kind = "contract"`, `channel` ou `resource`, também encontra os lados provisionadores pelo contrato.

`meada status` lista contratos órfãos por tipo e broker (ou tipo de recurso): provisionadores
sem uso e usos sem provisionador. Só há linhas de órfãos quando o repositório ou workspace
consultado contém pelo menos um provisionador e um uso daquele grupo. Um repositório só de
código ou só de IaC não gera essas linhas; `search` e `context` marcam o próprio lado com
`no provisioner in the workspace` ou `no use in the workspace`. As ligações de provisionamento,
inclusive padrões e módulos raiz independentes, determinam se há contraparte. Os avisos
existentes de recursos declarados sem escritor e leitor continuam separados.

Um uso que o Meada só infere pelo nome de uma chave (um valor simples sob `queue`, `topic`,
`bucket`, `table` ou um alias tipado, `inferred`) não abre o escopo de órfãos do seu grupo e
nunca vira linha de órfão: uma coincidência de nome não prova nem o uso nem a falta dele. Ele
continua ligado ao provisionador do mesmo nome, que por isso não é um `no use`, e o `context`
continua a mostrá-lo com `[inferred]`. Uma linha `no provisioner` termina com o nível do seu lado
entre colchetes, como o `context` o escreve (`[exact]`, `[declared]`), e as linhas `uses` do
`meada export` trazem `level`.

### Provisionadores Terraform

Um `resource` `aws_sqs_queue` provisiona `channel::sqs::<nome>` pelo argumento `name`;
um `aws_s3_bucket` provisiona `resource::s3::<nome>` pelo argumento `bucket`. A tabela de tipos
em `src/iac/types.toml`, embutida no binário, também lê nomes em ARNs e URLs. Um `[[resource]]`
com `kind = "s3"` e o mesmo `id` participa desse contrato, mantendo o nível `declared`.

CloudFormation e SAM usam os mesmos contratos para SQS, SNS, EventBridge, Kinesis, S3,
DynamoDB (Table, GlobalTable e SimpleTable), RDS (instância ou cluster), ElastiCache
(cluster ou grupo), SSM e Secrets Manager, quando o nome físico é conhecido. A propriedade
que define esse nome não cria um uso do próprio recurso; segredos contribuem só com a identidade.

Cada produtor, consumidor, escritor e leitor se liga a cada provisionador, identificado pelo
endereço e pelo módulo raiz. Duas raízes como `envs/dev` e `envs/prod` não são ambíguas entre si.
Nomes escritos dão `exact`; nomes que passam por valores sobrescrevíveis dão `inferred`, com
as variáveis e suas origens no motivo. `context` no bloco lista os lados do contrato; no produtor,
lista os provisionadores. `trace` segue do bloco para o código com `down` e do código para a
infraestrutura com `up`. O export mantém `format: 1` e acrescenta `iac_nodes`, `provisioners` e
`provisions`, omitidos quando vazios. Tipos não listados e blocos `data` não provisionam contratos.
Um padrão
com pelo menos três letras ou dígitos literais (separadores não contam) e curingas nos limites de
segmentos (`-`, `_`, `.`, `/`) se liga a um nome literal em `inferred`, ou `ambiguous` quando vários
nomes correspondem. Padrões nunca se ligam a padrões. O grafo da console lista os provisionadores e
suas ligações próprias.

### SDKs AWS em Java, Go e C#

As operações abaixo criam lados de contratos sem declaração em `meada.toml`. O cliente precisa vir de um construtor do SDK, com cada salto usado para comprovar essa origem resolvido por uma chamada exata. Parâmetros, locais e nomes de tipos que escondem o cliente ou seu construtor não herdam sua origem. O SQS mantém as formas anteriores em Java, Go e C#: um campo, parâmetro ou local tipado que o código declara com o tipo de cliente do SDK (`SqsClient` ou `SqsAsyncClient`, `*sqs.Client`, `IAmazonSQS` ou `AmazonSQSClient`) é um cliente, inclusive injetado; um tipo do repositório com esse nome não é. Um nome literal liga ao nome físico provisionado; uma leitura do ambiente fica sem resolução e conserva `env:NOME` no export, inclusive em produtores. O Meada nunca lê o ambiente do processo.

Leituras do ambiente por aliases de importação em Go e C# mantêm o nome da variável. Um pedido guardado por nome fica sem resolução quando há escrita no campo que nomeia o recurso, inclusive por um alias.

| Serviço | Operações e direção |
| --- | --- |
| SQS | `SendMessage`, `SendMessageBatch` produzem; `ReceiveMessage` consome, pelo `QueueUrl` |
| SNS | `Publish`, `PublishBatch` produzem, pelo `TopicArn`; uma publicação para telefone ou endpoint não nomeia tópico |
| EventBridge | `PutEvents` produz em cada `Entries[].EventBusName`, `default` quando omitido |
| Kinesis | `PutRecord`, `PutRecords` produzem, pelo `StreamName` |
| S3 | `PutObject`, `DeleteObject` escrevem; `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` leem, pelo bucket |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem` e as entradas `Put`, `Update` e `Delete` de `TransactWriteItems` escrevem; `GetItem`, `Query`, `Scan`, `BatchGetItem`, as entradas `Get` de `TransactGetItems` e as entradas `ConditionCheck` de `TransactWriteItems` leem, pelo `TableName`, por todas as chaves de `RequestItems` ou pelo `TableName` de cada entrada de `TransactItems` |
| SSM | `GetParameter`, `GetParameters` leem; `PutParameter`, `DeleteParameter`, `DeleteParameters` escrevem, por `Name` ou todos os `Names` |
| Secrets Manager | `GetSecretValue`, `BatchGetSecretValue` leem; `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` escrevem, por `SecretId`, todos os `SecretIdList` ou `Name` |

Os nomes das operações usam a grafia Go; Java começa com minúscula e .NET aceita as formas síncronas e `Async`. Em .NET, S3 usa `BucketName` e `GetObjectMetadata` para `HeadObject`. ARNs reduzem ao mesmo nome físico da infraestrutura. `GetParametersByPath` fica sem resolução porque `Path` é uma hierarquia, não um parâmetro. Operações de controle de RDS e ElastiCache não comprovam leitura ou escrita dos dados. Um recurso sem nome nunca liga; um lado declarado em `meada.toml` mantém o nível `declared`.

### GCP e Cloud Run

Terraform/OpenTofu reconhece `google_pubsub_topic` e `google_pubsub_subscription` como canais
`pubsub`, `google_storage_bucket` como `gcs`, `google_sql_database_instance` e
`google_sql_database` como `cloudsql`, e `google_secret_manager_secret` como
`gcpsecretmanager`. O nome vem de `name`, exceto no segredo, que usa `secret_id`.
Instâncias Cloud SQL usam `<instância>`; bancos usam `<instância>/<banco>`, com a instância
literal ou uma referência resolvida a uma instância conhecida. Um banco não provisiona sua
instância. Projetos ficam fora da identidade.

Um tópico usa `channel::pubsub::<id>`; uma assinatura usa
`channel::pubsub::subscriptions/<id>`, mesmo quando ambos têm o mesmo id. A assinatura liga
consumidores de seu destino aos produtores do tópico indicado por `topic`: `exact` sem
filtro, `inferred` com filtro ou nomes sobrescrevíveis. Uma assinatura sem tópico resolvido
não inventa uma ligação nem um tópico com seu nome.

`gs://<bucket>/objeto`, `projects/<projeto>/topics/<id>`,
`projects/<projeto>/subscriptions/<id>` e `projects/<projeto>/secrets/<id>` (também com
`/versions/<versão>`) são usos de configuração `exact`. Os nomes simples sob as chaves finais
`pubsub_topic`, `pubsub_subscription`, `gcs_bucket`, `cloudsql_instance`, `cloudsql_database`
e `gcpsecretmanager_secret` são `inferred`; `cloudsql_database` exige `<instância>/<banco>`.
As chaves genéricas `topic` e `bucket` conservam seus significados SNS e S3. Identificadores
malformados, valores ocultados ou cortados e uma simples igualdade de texto não criam usos.

`google_cloud_run_service`, `google_cloud_run_v2_service` e `google_cloud_run_v2_job` são
workloads: cada contêiner guarda sua imagem e seu ambiente. Imagens ligam ao repositório que
as publica; um padrão sobrescrevível enfraquece a ligação. Leituras de ambiente só recebem
valores do contêiner que executa seu código e ficam `inferred`. Referências Secret Manager
continuam alvos não resolvidos, sem ler o conteúdo do segredo. Busca, `context`, `trace` nos
dois sentidos, órfãos por escopo e export incluem os novos contratos e runners; entradas de
ambiente e valores de configuração ficam fora do export.

### Clientes dos SDKs GCP

Os cinco grupos de SDKs GCP (JavaScript/TypeScript, Python, Java, Go e C#) reconhecem as formas documentadas de publicação e recebimento por assinatura Pub/Sub, leitura/escrita no Cloud Storage e acesso ao Secret Manager. O [guia do usuário](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.md#uso-dos-sdks-gcp-em-cinco-grupos-de-linguagens) lista as operações e os construtores exatos. Tópicos e assinaturas mantêm identidades distintas, inclusive na resolução do ambiente da carga; uma leitura do ambiente sozinha continua não resolvida.

### Canais, dados, segredos e assinaturas AWS

A tabela Terraform/OpenTofu também reconhece os tipos abaixo. ARNs são reduzidos ao nome
físico, incluindo caminhos de parâmetros e segredos; o sufixo de seis caracteres do ARN do
Secrets Manager é removido. As mesmas regras de provisionamento, `context`, `trace` e export
valem para todos esses contratos.

| Tipo Terraform | Nome físico | Contrato |
| --- | --- | --- |
| `aws_sns_topic` | `name` | `channel::sns` |
| `aws_cloudwatch_event_bus` | `name` | `channel::eventbridge` |
| `aws_kinesis_stream` | `name` | `channel::kinesis` |
| `aws_dynamodb_table` | `name` | `resource::dynamodb` |
| `aws_db_instance` | `identifier` | `resource::rds` |
| `aws_rds_cluster` | `cluster_identifier` | `resource::rds` |
| `aws_elasticache_cluster` | `cluster_id` | `resource::elasticache` |
| `aws_elasticache_replication_group` | `replication_group_id` | `resource::elasticache` |
| `aws_ssm_parameter` | `name`, preservando `/` inicial | `resource::ssm` |
| `aws_secretsmanager_secret` | `name` | `resource::secretsmanager` |

Um parâmetro SSM `SecureString` tem `value` armazenado como `redacted` em HCL e no JSON de Terraform/OpenTofu. Quando `type` é uma
expressão que o leitor de valores não resolve, o valor também é ocultado por precaução.

`aws_sns_topic_subscription` com `protocol = "sqs"` liga consumidores da fila aos produtores
do tópico. `aws_cloudwatch_event_rule` e `aws_cloudwatch_event_target` fazem o mesmo do bus para
a fila; sem `event_bus_name`, usam `default`. Sem filtro, o nível é `exact`; com `filter_policy`
ou `event_pattern`, é `inferred`. Nomes sobrescrevíveis também dão `inferred`, com as variáveis
no motivo. O motivo identifica a assinatura ou o target e a regra. Endpoints não resolvidos,
protocolos diferentes e targets sem uma regra única no mesmo bus não criam ligações.
Regras somente com agendamento não roteiam eventos publicados no bus.

`context` lista produtores e consumidores através da assinatura. `trace` sobe do consumidor
até o produtor e desce do provisionador até os consumidores; um canal de origem `sns`,
`eventbridge` ou `kinesis` também pode iniciar o trace. Os resumos entram no array opcional `subscriptions`, com origem, destino, nó e raiz.
As ligações entram em `links`, mantendo
as ligações de provisionamento separadas em `provisions`. `meada status` lista contratos
provisionados sem uso e usos sem provisionador, por kind e broker, somente quando o workspace
contém ao menos um provisionador e um uso daquele grupo. Os outros formatos IaC continuam
como nós e referências; esta tabela de contratos cobre arquivos `.tf`, `.tf.json`, `.tofu` e `.tofu.json`.

## Kubernetes e Kustomize

Todo documento YAML ou JSON com `apiVersion`, `kind` e `metadata.name` é um nó IaC, com endereço `<Kind>[.<grupo>]/<namespace>/<nome>` (`Deployment.apps/shop/api`, `Service/shop/api`; o grupo core não escreve grupo). Um kind de escopo de cluster não tem namespace no endereço (`ClusterRole.rbac.authorization.k8s.io/reader`), nem um objeto que não define nenhum e a que nenhum overlay dá um. Um template Helm (um nome com `{{`) e um arquivo do Kustomize não são objetos.

Uma pasta com `kustomization.yaml` que nenhum outro kustomization nomeia como base é um overlay e uma unidade de avaliação. Os `resources`, `bases` e `components` dela que nomeiam um arquivo ou outro kustomization dentro do repositório são instanciados sob ela, cada nó rotulado pelo overlay (`root overlays/dev`), com o `namePrefix`, o `nameSuffix`, o `namespace` e as `images` de cada kustomization no caminho aplicados, os da base primeiro e os do overlay por último: uma base usada por dois overlays é um nó em cada, com o nome e o namespace dele. Um `..` que fica dentro do repositório é seguido (`../../base`). Uma entrada remota (uma URL, `github.com/org/repo//caminho?ref=v1`), acima da raiz do repositório, absoluta, ausente, alcançada por um symlink, em ciclo ou aninhada além de 16 níveis não é seguida, nunca é baixada, e o `meada status` a conta (`entradas do kustomize não seguidas: N`). Os manifestos que nenhum kustomization lista, ou usa como patch, são simples: os manifestos de uma pasta são uma unidade de avaliação, rotulada pela pasta.

| Referência | Guardada como |
| --- | --- |
| Um Service para os donos de pod template (Deployment, StatefulSet, DaemonSet, Job, CronJob, Pod) cujos labels do pod contêm o selector dele | `via selector app=api` |
| Uma carga de trabalho para os ConfigMaps e Secrets de `env`, `envFrom` e volumes | `via env DATABASE_URL`, `via envFrom`, `via volume settings` |
| Uma carga de trabalho para o `serviceAccountName` dela | `via serviceAccountName` |
| Um Ingress para os Services das regras e do backend padrão | `via backend shop.example.com/api`, `via defaultBackend` |

Uma referência fica num namespace e numa unidade, e nomeia o objeto como foi escrito, antes do prefixo e do sufixo do overlay, que o overlay aplica depois aos dois lados. Toda referência é `exact`. `search` com kind `iac` e `context` respondem por esses nós como pelos do Terraform, e `context` numa carga de trabalho acrescenta `images:`, o que cada contêiner roda depois das `images` do overlay (`web: registry.example.com/shop/api:1.4.0`). Patches, geradores, `commonLabels`, templates Helm e `kind: List` não são aplicados nem lidos, e os objetos ficam fora do `meada export`.

### Crossplane

Objetos Crossplane são objetos Kubernetes, e o `context` diz o que cada um é para o Crossplane numa linha `role:`: uma definição de recurso composto (XRD), uma composition (as duas sem namespace), um claim ou um recurso composto (um objeto do kind de claim ou composto que um XRD do repositório define), um recurso gerenciado (um objeto com `spec.forProvider`) ou um recurso criado por uma composition. Cada recurso que uma composition cria a partir de uma `base` literal, em `spec.resources` ou no `input.resources` de um passo de pipeline que roda a função patch-and-transform, é um nó `<composition>/<recurso>` do kind da base, com o nome da anotação `crossplane.io/external-name` ou do `metadata.name` dela; o nome de um recurso gerenciado também é o external name.

| Referência | Guardada como |
| --- | --- |
| Um claim para o XRD do kind composto dele | `via composite XPostgreSQLInstance.database.acme.io` |
| Um recurso composto para o XRD dele | `via kind XNetwork.network.acme.io` |
| Um claim ou recurso composto para a composition que o `compositionRef` nomeia | `via compositionRef` |
| Um XRD para a composition padrão e a imposta | `via defaultCompositionRef`, `via enforcedCompositionRef` |
| Uma composition para o XRD do `compositeTypeRef` | `via compositeTypeRef XPostgreSQLInstance.database.acme.io` |
| Uma composition para cada recurso que cria | `via resources`, `via pipeline <passo>` |
| Um recurso criado ou gerenciado para a definição do kind dele (um XRD ou um `CustomResourceDefinition`) | `via kind Bucket.s3.aws.upbound.io` |

Toda referência é `exact` e fica numa unidade, como as do Kubernetes: um claim numa pasta diferente da do XRD mantém o papel e não referencia nada. O `trace` a partir de um claim desce até o XRD, a composition e os recursos que ela cria. Patches e funções não são avaliados: um nome que um patch (ou um patch set) define, ou uma base sem nome, é o padrão `*`, e um passo que roda qualquer outra função (Go templating, KCL) não cria nó.

### Compose

Um arquivo Compose (`compose.yaml`, `compose.yml`, `docker-compose.yaml` ou `docker-compose.yml`) com o arquivo de override na mesma pasta é uma unidade de avaliação, enraizada na pasta. Cada serviço é um nó IaC `services.<nome>` do tipo `service`. `depends_on`, `links` e redes compartilhadas (`networks:`; um serviço sem nenhuma está em `default`) são referências, com `via depends_on`, `via links` ou `via networks.<nome>`. `${VAR:-padrão}` resolve para o padrão e o `context` o marca `(overridable)`; `${VAR}` fica no texto como parte não resolvida, marcada `(unresolved)`. O `context` mostra `image:` e `build:` do serviço, guardados para o contrato de imagem, com o Dockerfile que o build nomeia (`dockerfile` ou `dockerfile_inline`); um contexto de build só é guardado como caminho dentro do repositório (caminho absoluto, URL, um que sobe para fora da raiz ou um com parte não resolvida não são).

### CloudFormation e SAM

Um template qualificado chamado `compose.yaml` mantém seus nós e referências no índice inicial e nas atualizações; o nome do arquivo não o torna Compose.

Um template YAML (as tags curtas `!Ref`, `!Sub`, `!GetAtt`, `!Join` e as demais são lidas) ou JSON com um mapa `Resources` que traga `AWSTemplateFormatVersion` ou a transformação SAM (`AWS::Serverless-2016-10-31`) é uma unidade de avaliação, enraizada no seu caminho. Cada recurso é um nó IaC endereçado pelo seu id lógico dentro do template, e seu tipo (`AWS::SQS::Queue`, `AWS::Serverless::Function`: tipos SAM são nós como quaisquer outros) é o tipo do nó. `Ref`, `Fn::GetAtt` (com o atributo no `via`: `via Fn::GetAtt.Arn`), as variáveis de `Fn::Sub` e `DependsOn` são referências, todas `exact`. O nome físico de fila, tópico, bucket, tabela, função, stream, segredo e mais alguns tipos é avaliado dentro do template: o padrão de um parâmetro (o `context` o lista em `overridable:`), `Ref`, `Fn::Sub`, `Fn::GetAtt` e `Fn::Join`. Um pseudo parâmetro (`AWS::Region`), um import (`Fn::ImportValue`), qualquer outra função e um parâmetro sem padrão ficam como partes não resolvidas: o nome vira um padrão, com um `*` para cada uma. Um parâmetro com `NoEcho` tem o padrão guardado como `redacted` entre os valores de configuração e nenhum nome passa por ele. Um template alterado é avaliado de novo sozinho. Stacks aninhadas, macros e condições não são seguidas.

### Serverless Framework

Um `serverless.yml`, `serverless.yaml` ou `serverless.json` com um `service` é uma unidade de avaliação, enraizada no seu caminho. Cada função é um nó IaC `functions.<nome>` do tipo `Serverless::Function`, com o nome do seu `name` ou `<service>-<stage>-<função>` (o estágio substituível por `--stage`, `opt:stage`), e seu handler e seus eventos são guardados e mostrados pelo `context` (`handler:`, `event:`: `http GET /orders`, `sqs Queue.Arn`, `schedule rate(5 minutes)`). Uma função referencia, `exact`, os recursos que seu corpo nomeia com `Ref`, `Fn::GetAtt` ou `Fn::Sub`. A seção `resources` é lida como CloudFormation: cada recurso é um nó no seu id lógico, com os nomes e referências descritos acima. `${self:...}` resolve para o valor naquele caminho do arquivo, e o padrão de `${opt:...}` resolve e é marcado `(overridable)`, pois quem faz o deploy o passa na linha de comando; `${env:...}`, `${ssm:...}`, `${sls:...}`, `${file(...)}` e as demais fontes ficam como partes não resolvidas: um `*` no nome, `${...}` num texto guardado. Um valor que nomeia a si mesmo, direto ou em ciclo, fica sem resolver, e `${self:provider.stage}` sem estágio no arquivo é `dev`, como no nome de uma função. Um serviço alterado é avaliado de novo sozinho. `serverless.ts`, plugins e valores de outros arquivos não são lidos.

## Comunidades e processos

Uma comunidade reúne símbolos que se chamam mais entre si do que ao restante. `meada export` calcula esses grupos com Leiden, sem aleatoriedade e com aritmética inteira. O nome vem da pasta da maioria dos membros (`src`, `src#2`), igual em toda máquina.

Cada ligação tem nível `exact`, `declared`, `inferred`, `decided` ou `ambiguous`. Uma URL não resolvida só vira ligação após uma decisão, com nível `decided`. Uma rota cujo local de serviço é desconhecido fica fora do mapa.

Um processo é a árvore de chamadas iniciada por um ponto de entrada, atravessando repositórios. São pontos de entrada as rotas, os handlers de canais e as funções ou métodos exportados sem chamadores no próprio repositório. Também entram:

- Python: `if __name__ == "__main__":` (símbolo `__main__`) e funções de `[project.scripts]` no formato `módulo:função`. Exportados são nomes públicos, sem `_` inicial, ou os listados em `__all__` literal.
- Rust: `fn main` de cada binário em `src/main.rs`, `src/bin/` ou `[[bin]] path`.
- Go: `func main` em um `package main`.
- Swift: um tipo marcado `@main` (pelo seu método `main()` quando ele declara um) e o código de topo de um arquivo `main.swift`, num símbolo `main` dono das chamadas escritas ali.

`trace` com `process::<identidade do ponto de entrada>` lê a árvore. `meada export` inclui cada processo. `search` com tipo `community` encontra comunidades pelo nome; com tipo `process`, encontra processos pelo rótulo do ponto de entrada; com tipo `duplicate`, lista os símbolos cujo texto é idêntico em dois ou mais lugares do workspace (entre repositórios ou dentro de um), em grupos ordenados pelo tamanho do trecho, achados pelo nome ou pelo caminho de um membro. O export traz o `hash` BLAKE3 de cada símbolo; cópias quase iguais (uma diferença de espaço em branco) não são duplicatas. `context` mostra a comunidade do símbolo (ou `community: none`) e seus processos, percorrendo chamadas e ligações no sentido dos chamadores.

## Limites e próximos passos

Linguagens e clientes HTTP ou de mensagens fora das listas acima ainda não têm suporte. O [ADR 0003](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr/0003-v1-scope-and-roadmap.md) define o escopo e a ordem do trabalho. Consulte o [roadmap](roadmap.md), as [decisões](https://github.com/ClaudioMascaro/meada/blob/main/docs/adr) e o [CHANGELOG](https://github.com/ClaudioMascaro/meada/blob/main/CHANGELOG.md).

## AWS SDK em código

Leituras de recursos mantêm cada chamada SDK como lado distinto em JS, TS e Python. `trace` para cima alcança os escritores; a partir de um escritor, para baixo alcança a função leitora e continua pelas chamadas dela, com o local real de cada operação.

TypeScript e JavaScript reconhecem comandos v3 enviados com `cliente.send(new Comando(...))`,
com cliente e comando importados do pacote AWS correspondente. Python reconhece métodos de
clientes criados por `boto3.client(...)` ou por uma `Session`. Os nomes podem vir de constantes
e dos argumentos de wrappers chamados exatamente. Um cliente recebido só por correspondência
de nome, ou escondido por um parâmetro, `catch`, desestruturação ou outro binding, não cria lado.

| Serviço | Produz ou escreve | Lê | Argumento do nome |
| --- | --- | --- | --- |
| SNS | `Publish`, `PublishBatch` | | `TopicArn` |
| EventBridge | `PutEvents` | | `Entries[].EventBusName`, `default` quando omitido |
| Kinesis | `PutRecord`, `PutRecords` | | `StreamARN` ou `StreamName` |
| S3 | `PutObject`, `DeleteObject` | `GetObject`, `HeadObject`, `ListObjects`, `ListObjectsV2` | `Bucket` |
| DynamoDB | `PutItem`, `UpdateItem`, `DeleteItem`, `BatchWriteItem`, `TransactWriteItems` | `GetItem`, `Query`, `Scan`, `BatchGetItem`, `TransactGetItems` | `TableName`, chaves de `RequestItems`, ou tabelas de `TransactItems` |
| SSM | `PutParameter`, `DeleteParameter`, `DeleteParameters` | `GetParameter`, `GetParameters`, `GetParametersByPath` | `Name`, `Names`, `Path` |
| Secrets Manager | `PutSecretValue`, `UpdateSecret`, `CreateSecret`, `DeleteSecret` | `GetSecretValue`, `BatchGetSecretValue` | `SecretId`, `SecretIdList`, `Name` (`CreateSecret`) |

Em JavaScript, cada operação acima tem o sufixo `Command`; em boto3, usa snake case
(`put_object`, `get_secret_value`). SNS, EventBridge e Kinesis são produtores; S3, DynamoDB,
SSM e Secrets Manager usam os lados escritor e leitor de recursos. Os nomes literais e ARNs compatíveis se ligam à
infraestrutura pelo nome físico, sem conta ou região. Lados de SDK literais se ligam em
`exact`, inclusive a outros lados de SDK do mesmo repositório; um lado declarado em
`meada.toml` conserva `declared`. Valores de infraestrutura substituíveis continuam `inferred`.

Uma leitura como `process.env.BUCKET` ou `os.getenv("BUCKET")` fica não resolvida, com
`via: "env:BUCKET"` no export e o nome da variável no contexto da função. Um produtor ou
escritor de SDK também tem `via`, opcional no formato 1; para um literal é `sdk:aws`.
Meada não lê o ambiente do processo. A construção isolada de um comando, um publish SNS para
telefone ou endpoint, os clientes de alto nível `boto3.resource` e `DynamoDBDocumentClient`
e operações fora da tabela não são reconhecidos por estas formas.

`GetParametersByPath` consulta uma hierarquia, por isso seu lado fica não resolvido; o caminho não é tratado como nome de um parâmetro. Um `ConditionCheck` de `TransactWriteItems` é leitor, mesmo quando a transação também escreve outras tabelas. Chaves computadas de `RequestItems` preservam o literal ou o nome da variável de ambiente.

## Imagens e código das cargas de trabalho

O contrato `image::<registry>/<repository>` liga a carga de trabalho ao repositório cuja
configuração constrói a imagem (#662). `nginx:1.27` vira `image::docker.io/library/nginx`;
`acme/api` vira `image::docker.io/acme/api`. Nomes ficam em minúsculas, sem tag nem digest.

Os construtores vêm de `docker/build-push-action` (`tags`), `docker/metadata-action` (`images`),
Compose com `build` e `image`, artefatos Skaffold (inclusive vários documentos YAML), destinos
Jib literais em Maven/Gradle e comandos ko `build --bare` ou `publish --bare` com
`KO_DOCKER_REPO` definido no workflow. Imagens-base não são destinos. Comandos ko com outro
modo de nomeação e expressões Gradle não lidas não criam um construtor por suposição.

Os executores são containers e init containers de cargas Kubernetes, com os overrides de
imagem do Kustomize aplicados, serviços Compose e `image.repository` de arquivos de valores
Helm. Templates Helm não são renderizados. `context` mostra a imagem e os repositórios
construtores; `trace` percorre a relação nos dois sentidos, até as entradas do código.
Overlays que usam a mesma carga e a mesma imagem preservam um executor e sua ligação por
raiz de avaliação. Evidências repetidas da mesma imagem na mesma carga e raiz formam um só lado.
Um build Compose local executa diretamente o código do mesmo repositório que o seu Dockerfile
copia do contexto (fontes de `COPY`, `ADD` e `RUN --mount=type=bind`, em todos os estágios) em
`exact`, mesmo sem `image`, dentro da raiz e sem atravessar links simbólicos; em `inferred` quando
o contexto ou o caminho do Dockerfile passou por um valor sobrescrevível. Um Dockerfile que não
pode ser lido deixa o contexto inteiro em `inferred`, e o motivo diz isso. O `.dockerignore` não
é lido.

Literais ligam em `exact`; defaults Compose e valores Helm em `inferred`, com sua origem no
motivo. Trechos não resolvidos permanecem padrões e só correspondem a nomes literais nos
limites de segmentos; vários candidatos ficam `ambiguous` e podem receber uma decisão.
`status` só lista imagens órfãs quando o escopo visível tem construtores e executores.
Sem um desses lados, `search` e `context` marcam o lado sem contraparte. A exportação mantém
formato 1, com `at` nos construtores e `executions` para os caminhos diretos; o grafo do console
mostra os contratos de imagem, as ligações e o código das cargas de trabalho.

### Nomes parcialmente resolvidos

Um canal, recurso ou imagem com partes desconhecidas preserva um padrão como `*-orders`.
`${terraform.workspace}-orders` corresponde a `prod-orders` em `inferred`: são necessários
ao menos três letras ou dígitos literais (separadores não contam), e cada `*` deve ocupar segmentos
inteiros delimitados por `-`, `_`, `.` ou `/`. Um padrão nunca corresponde a outro padrão. Vários
nomes literais compatíveis ficam `ambiguous`, todos listados, e viram itens abertos. `meada decide`
grava a escolha em `meada.decisions.toml`; o vínculo escolhido fica `decided`. Decisões de
provisionamento pertencem ao lado que contém o padrão e conservam sua raiz de avaliação.

Itens em chaves de configuração usam metadados de arquivo, linha, chave e contrato, inclusive quando o valor é `redacted` ou cortado.

Um builder de imagem com padrão e vários nomes literais de runners também mantém seu próprio item aberto no repositório que faz o build.

## Locais de recursos declarados

O `via` de `[[resource]]` aceita símbolos (`src/orders.ts::cancelOrder`), endereços IaC e
`<arquivo>#<caminho da chave>`. O lado fica no bloco ou na chave, alcançável por `context` e
`trace`, e suas ligações continuam `declared`. Os endereços incluem Terraform/OpenTofu
(`aws_sqs_queue.orders`), Kubernetes/Crossplane (`Deployment.apps/shop/api`), Compose
(`services.api`), CloudFormation/SAM (`template.yaml::Orders`) e Serverless
(`serverless.yml::functions.api`). Use `<arquivo>::<endereço>` para distinguir arquivos. O local identifica o bloco de origem;
o qualificador ` (root ...)` das consultas IaC não é uma forma de `via`.
Um endereço que corresponde a vários blocos de origem precisa do qualificador de arquivo;
sem ele, o `via` é obsoleto.

Caminhos de chave usam pontos, índices (`groups[0].name`) e chaves entre aspas JSON
(`metadata.labels["app.kubernetes.io/name"]`). Em listas, `[chave=valor]` seleciona os itens
pela igualdade do valor completo, por exemplo
`values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support`.
A localização vem de valores indexados e seus caminhos pais.
Valores nulos e contêineres vazios não têm valor indexado; seu `via` é obsoleto.
Um seletor pode encontrar vários itens; cada chave encontrada recebe um lado. Valores
marcados como `redacted` ou cortados não servem para selecionar um item. Expressões e templates
HCL não satisfazem a igualdade de um seletor. Os arquivos seguem as regras do
índice, incluindo `.meadaignore`; nenhum arquivo é aberto por causa de um `via`.

Um `via` sem correspondência é obsoleto e aparece nas linhas existentes de `meada status`.
Sem nenhum `via` válido, o lado volta ao repositório inteiro. Isso permite declarar o vínculo
entre rótulos em valores Helm e regras de roteamento sem renderizar templates.

```toml
[[resource]]
kind = "routing"
id = "team-b"
role = "write"
via = ["values.yaml#groups[name=availability].rules[alert=ApiDown].labels.support"]
```

## Funções e tarefas AWS

Lambda em Terraform/OpenTofu executa `image_uri`; SAM executa `ImageUri`, e tarefas ECS
executam as imagens de cada container em `container_definitions`, inclusive via `jsonencode`.
Esses executores entram nos contratos de imagem, no contexto, nos rastros e nos órfãos.
O ambiente acompanha o container e a raiz de avaliação, sem alcançar código de outra imagem.

Um `filename` local de Lambda ou `CodeUri` de SAM liga diretamente o código do mesmo
repositório. Uma referência exata a `data.archive_file.<nome>.output_path` pode seguir somente
seu `source_dir` ou `source_file` local explícito. Arquivos ZIP ficam como locais opacos;
seu conteúdo não é extraído. Um handler Serverless como `worker/send.dispatch` seleciona
um único arquivo local existente (`.ts`, `.js`, `.mjs`, `.cjs` ou `.py`); extensões ambíguas
ficam sem resolver. Caminhos novos devem ser relativos, sem nenhum componente `..` e sem
atravessar links simbólicos. S3, URLs e código remoto não viram caminhos locais.

SAM combina `Globals.Function.Environment.Variables` com o ambiente da função; Serverless
combina `provider.environment` com `functions.<nome>.environment`. A função prevalece,
mesmo quando o valor que substitui o padrão não pode ser lido. Terraform Lambda e ECS também
fornecem seus ambientes. Referências conhecidas a recursos podem carregar a identidade do
contrato sem inventar um ARN ou URL do deploy. Segredos, referências dinâmicas e valores
externos continuam sem conteúdo. A leitura em código resolve em `inferred`, com workload e
variável no motivo; `context` e `trace` mostram tanto a execução por imagem como o caminho direto.

Referências como `aws_sqs_queue.orders.url` e `aws_s3_bucket.assets.bucket` identificam o
contrato sem fornecer o URL do deploy; atributos de conteúdo como `value` continuam sem resolver.

## Ambiente dos workloads

Uma leitura de ambiente que nomeia uma fila, tópico ou recurso, como `process.env.QUEUE_URL`,
é resolvida somente pelas entradas do workload que executa aquele código. O vínculo de
execução vem da imagem publicada pelo repositório ou de um caminho local verificado do workload.
Containers diferentes mantêm seus próprios ambientes. A resolução fica em `inferred`, com
o workload e a variável no motivo.

O Kubernetes fornece valores literais, referências a chaves de ConfigMap ou Secret e `envFrom`;
o Compose fornece entradas em mapa ou lista e defaults como `${QUEUE_URL:-orders}`.
`context` no workload mostra os nomes, os destinos e o valor disponível. Um Secret continua
`redacted`, e uma referência ausente ou um valor não resolvido não cria vínculo. Nomes só
coincidem dentro do escopo do workload: os valores vêm do IaC e da configuração dos
repositórios, nunca do ambiente de um processo.

## Alertas e rotas do Alertmanager

O contrato `alert::<chave>::<valor>` liga os rótulos de regras Prometheus e Loki
(inclusive `PrometheusRule`) e Grafana aos matchers de rotas do Alertmanager, em arquivos
de configuração e objetos `AlertmanagerConfig`. Igualdade gera um link `exact`; uma
expressão regular gera `inferred`. As condições de uma rota são verificadas em conjunto.
Uma condição negada (`!=`, `!~`) só filtra a rota: não cria lado nem link para o valor que
ela exclui.

Uma expressão regular que o `regexp` do Go não compila (um grupo `(` sem fechar, um `*` no
início, um intervalo invertido `[z-a]`, um lookahead `(?=`) faz o Alertmanager recusar a
configuração inteira, então o Meada lê essa configuração, ou esse objeto `AlertmanagerConfig`,
como ilegível: ela não cria lado de alerta nem link, e `meada status` conta o arquivo em
`ilegíveis`. A mesma configuração com a expressão corrigida gera links normalmente. Uma
expressão que o Meada deixa sem correspondência só por seus próprios limites, ou por não ler a
sintaxe, vale para o Alertmanager até onde o Meada sabe: o matcher aparece como órfão e as
demais rotas continuam gerando links.

`search` com kind `alert` encontra o contrato; `context` na rota mostra seu receiver,
e `trace` percorre da regra à rota e de volta. `meada export` inclui emissores, matchers
e links. `meada status` só lista órfãos quando o escopo visível tem os dois lados;
sem isso, `search` e `context` indicam o lado ausente. Valores Helm que ainda dependem
de templates não são emissores: Meada não renderiza templates.

Regras Grafana também são lidas de exports JSON e de blocos `rule` de `grafana_rule_group` em Terraform/OpenTofu, com rótulos literais.

Uma rota raiz sem matchers não cria um lado de alerta: não há chave e valor para o contrato. O grafo descreve os matchers escritos, sem calcular notificações finais, inibições ou horários.

Prometheus e Loki também emitem o `alertname` literal do nome da regra, sobrescrevendo um rótulo com esse nome. Rótulos que possam vir da expressão permanecem desconhecidos, não vazios. Arquivos sob `templates/` não são renderizados nem tratados como emissores de alertas.

### OpsGenie e PagerDuty em Terraform

Tags literais `chave:valor` de `datadog_monitor` são emissores. Condições de tags em
`opsgenie_team_routing_rule` e condições PCL de
orquestrações PagerDuty, são matchers. Igualdade é `exact`; regex, prefixo e
contenção são `inferred`. As condições e a ordem das regras são avaliadas juntas;
valores desconhecidos ou operações não lidas não escolhem um destino por palpite.

Tags do Datadog chegam ao OpsGenie. Rótulos de regras só chegam a tags por um receiver
OpsGenie selecionado no Alertmanager que configure `tags` explicitamente. Templates
simples de `.CommonLabels.<chave>` ou `.GroupLabels.<chave>` usam rótulos fixados pelo
`group_by` efetivo. Um receiver PagerDuty v2 pode encaminhar esses rótulos por `details`,
que vira `event.custom_details.<chave>`; não há encaminhamento implícito de tags ao
PagerDuty. Templates não lidos e encaminhamentos ausentes não geram links.

`context` mostra o escopo escrito da regra, e `trace` mantém o caminho até o bloco de
roteamento. Um matcher compatível não comprova entrega nem uma notificação final a
uma equipe. No exemplo público #638, a regra testa `service:app`: o monitor também
emite `support:team-b`, mas não existe ali um matcher para `support`. O YAML de
monitor não padronizado e os valores Helm originais continuam apenas configuração.

## Azure: contratos e Functions

A tabela Terraform/OpenTofu lê `azurerm_servicebus_queue`, `azurerm_servicebus_topic` e
`azurerm_servicebus_subscription` como canais `servicebus`; `azurerm_eventhub` como
`eventhubs`; `azurerm_storage_account` como recurso `azurestorage`,
`azurerm_storage_container` como `azureblob` e `azurerm_key_vault_secret` como `keyvault`.
A conta e o container com o mesmo nome continuam contratos distintos.

Uma assinatura tem identidade `channel::servicebus::subscription:<tópico>/<assinatura>`.
Ela liga seus consumidores aos produtores do tópico, com `exact` sem regra de filtro e
`inferred` quando uma regra é declarada. Filas e tópicos preservam `/` no nome. Os processors
C# já suportados continuam vinculados ao tópico, inclusive quando recebem uma assinatura.
IDs ARM completos e URLs de blob ou segredo reconhecidos dão usos `exact`; chaves finais
`servicebus_queue`, `servicebus_topic`, `servicebus_subscription` (valor `tópico/assinatura`),
`eventhubs_hub`, `azurestorage_account`, `azureblob_container` e `keyvault_secret` dão
`inferred`. Valores iguais sem essa evidência não ligam.

`azurerm_linux_function_app` lê o bloco Docker de `site_config.application_stack`, e ambos
os tipos Linux e Windows leem `zip_deploy_file` local e `app_settings`. O arquivo local
permanece dentro do repositório, sem `..` nem symlink; arquivos ZIP não são abertos. Um
pacote remoto não identifica código local. Referências Key Vault em app settings continuam
ocultas e sem valor resolvido. Os contratos e workloads participam de `context`, `trace`,
órfãos, provisionamento e export.

### SDKs Azure nos cinco grupos de linguagens

Construtores conhecidos dos SDKs Azure em JavaScript/TypeScript, Python, Java, Go e C# criam produtores/consumidores de Service Bus e Event Hubs, escritores/leitores de contêineres Blob e leitores de segredos Key Vault. O [guia do usuário](https://github.com/ClaudioMascaro/meada/blob/main/docs/user-guide.md#uso-dos-sdks-azure-nos-cinco-grupos-de-linguagens) lista os construtores e as operações. Receptores de assinatura mantêm tópico e assinatura na mesma carga, raiz e contêiner; nomes do ambiente sem essa evidência continuam não resolvidos. As formas C# existentes de Service Bus mantêm a identidade do tópico.

## Recursos personalizados e módulos de infraestrutura

Esta é uma lista fechada de 43 tipos de recursos personalizados, mais os papéis de Exchange e Binding do operador RabbitMQ. Tipos e grupos precisam coincidir exatamente. Outros CRDs continuam disponíveis em `search` com `kind = "iac"`, `context` e `trace`, sem receber um contrato presumido. O Meada lê os arquivos do repositório, sem consultar operadores ou baixar módulos.

| Tipo e grupo | Campo de identidade | Contrato |
| --- | --- | --- |
| `KafkaTopic.kafka.strimzi.io` | `spec.topicName` | `channel::kafka` |
| `Queue.rabbitmq.com` | `spec.name` | `channel::amqp` |
| `Queue.sqs.services.k8s.aws` | `spec.queueName` | `channel::sqs` |
| `Bucket.s3.services.k8s.aws` | `spec.name` | `resource::s3` |
| `Topic.sns.services.k8s.aws` | `spec.name` | `channel::sns` |
| `EventBus.eventbridge.services.k8s.aws` | `spec.name` | `channel::eventbridge` |
| `Stream.kinesis.services.k8s.aws` | `spec.name` | `channel::kinesis` |
| `Table.dynamodb.services.k8s.aws` | `spec.tableName` | `resource::dynamodb` |
| `DBInstance.rds.services.k8s.aws` | `spec.dbInstanceIdentifier` | `resource::rds` |
| `DBCluster.rds.services.k8s.aws` | `spec.dbClusterIdentifier` | `resource::rds` |
| `CacheCluster.elasticache.services.k8s.aws` | `spec.cacheClusterID` | `resource::elasticache` |
| `ReplicationGroup.elasticache.services.k8s.aws` | `spec.replicationGroupID` | `resource::elasticache` |
| `Parameter.ssm.services.k8s.aws` | `spec.name` | `resource::ssm` |
| `Secret.secretsmanager.services.k8s.aws` | `spec.name` | `resource::secretsmanager` |
| `PubSubTopic.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `PubSubSubscription.pubsub.cnrm.cloud.google.com` | `spec.resourceID` | `channel::pubsub` |
| `StorageBucket.storage.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcs` |
| `SQLInstance.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SQLDatabase.sql.cnrm.cloud.google.com` | `spec.resourceID` | `resource::cloudsql` |
| `SecretManagerSecret.secretmanager.cnrm.cloud.google.com` | `spec.resourceID` | `resource::gcpsecretmanager` |
| `Queue.sqs.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sqs` |
| `Bucket.s3.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::s3` |
| `Topic.sns.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::sns` |
| `Bus.cloudwatchevents.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventbridge` |
| `Stream.kinesis.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::kinesis` |
| `Table.dynamodb.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::dynamodb` |
| `Instance.rds.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::rds` |
| `Cluster.elasticache.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::elasticache` |
| `Parameter.ssm.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::ssm` |
| `Secret.secretsmanager.aws.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::secretsmanager` |
| `Topic.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Subscription.pubsub.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::pubsub` |
| `Bucket.storage.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcs` |
| `DatabaseInstance.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Database.sql.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::cloudsql` |
| `Secret.secretmanager.gcp.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::gcpsecretmanager` |
| `Queue.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Topic.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `Subscription.servicebus.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::servicebus` |
| `EventHub.eventhub.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `channel::eventhubs` |
| `Account.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azurestorage` |
| `Container.storage.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::azureblob` |
| `Secret.keyvault.azure.upbound.io` | `metadata.annotations["crossplane.io/external-name"]` | `resource::keyvault` |

Strimzi usa `metadata.name` somente quando `spec.topicName` está ausente. Config Connector usa `metadata.name` somente quando `spec.resourceID` está ausente. Um campo presente, mas desconhecido ou inválido, não ativa esse padrão. Nas famílias Crossplane listadas, a anotação explícita `crossplane.io/external-name` é obrigatória. O nome físico mostrado no nó continua separado do identificador normalizado do contrato.

Cloud SQL usa a identidade composta `<instância>/<banco>`: `SQLDatabase` lê `spec.instanceRef` e `Database.sql.gcp.upbound.io` lê `spec.forProvider.instance` ou `instanceRef`. Referências locais precisam resolver a um único objeto do grupo, tipo e namespace corretos. Assinaturas Pub/Sub têm contrato próprio `channel::pubsub::subscriptions/<nome>` e leem a origem em `spec.topicRef` ou `spec.forProvider.topic`/`topicRef`. Assinaturas Service Bus precisam do nome completo ou ARM ID que identifica tópico e assinatura, gerando `channel::servicebus::subscription:<tópico>/<assinatura>`. Um nome curto sem qualificação não basta.

Bases literais de Composition usam o mesmo inventário. Patches que podem mudar identidade, qualificação ou rota suprimem a projeção; patches desconhecidos, conjuntos ausentes ou duplicados e conjuntos aninhados também não estabelecem um contrato. Funções e patches não são executados.

### Topologia RabbitMQ

`Queue.rabbitmq.com` provisiona `channel::amqp::<spec.name>`. `Exchange.rabbitmq.com` fornece seu nome e tipo, sem virar canal. `Binding.rabbitmq.com` referencia `spec.source` e `spec.destination` no mesmo conjunto de avaliação, cluster, namespace e vhost; uma referência por `connectionSecret` fica separada de uma referência por nome de cluster. Somente destinos do tipo `queue`, com endpoints únicos e conhecidos, geram binding. `topic` usa `*` e `#`; `direct` aceita chaves sem curingas; `fanout` alcança todas as chaves. `headers`, tipos desconhecidos e destinos do tipo `exchange` não geram rota de fila. Estes últimos ainda têm referências navegáveis. O tipo ausente de Exchange é `direct`. Nomes e chaves AMQP ficam limitados a 255 bytes. O provisionamento de um publish conserva seu local real e o nível da rota, `exact` para chave literal e `inferred` para curinga.

### Módulos de registro

Somente estas seis origens são reconhecidas; o prefixo opcional `registry.terraform.io/` é aceito. A tabela lista entradas lidas e controles de criação.

| Origem | Entradas; controles |
| --- | --- |
| `terraform-aws-modules/sqs/aws` | `name`, `fifo_queue`, `use_name_prefix`; `create` |
| `terraform-aws-modules/sns/aws` | `name`, `fifo_topic`, `use_name_prefix`; `create` |
| `terraform-aws-modules/s3-bucket/aws` | `bucket`; `create`, `create_bucket`, `putin_khuylo` |
| `terraform-aws-modules/dynamodb-table/aws` | `name`; `create`, `create_table` |
| `terraform-aws-modules/lambda/aws` | `function_name`, `package_type`, `image_uri`, `environment_variables`, `source_path`, `local_existing_package`, `handler`; `create`, `create_function`, `create_package`, `create_layer`, `putin_khuylo` |
| `terraform-aws-modules/ecs/aws//modules/service` | `name`, `container_definitions` (`name`, `create`, `image`, `environment`, `secrets`); `create`, `create_service`, `create_task_definition` |

`create = false`, `count = 0` ou um controle de criação desconhecido não produzem lado. Nomes desconhecidos ficam como padrões seguros, sem nome físico inventado. SQS e SNS acrescentam `.fifo` quando solicitado; nomes gerados por `use_name_prefix` não criam provisionador literal. Lambda e ECS fornecem imagens e ambientes pelo mesmo leitor dos workloads nativos, mantendo os escopos por contêiner. Lambda aceita `source_path` escalar conhecido com `create_package = true`, dentro do repositório e sem `..` ou symlink; formas de lista/objeto e empacotamento S3 ficam sem alvo local. `local_existing_package` deve ser um arquivo existente e permanece opaco: não há extração de ZIP nem execução de empacotador. `create_layer = true` suprime a função. O código não presume o diretório do módulo como código da função.
