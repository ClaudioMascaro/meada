# prove.py run

differences found: 5

## meada

meada 1.1.0 (base) vs meada 1.1.0 (head)

Determinism: cold twice equal base True, head True; refresh equal base True, head True
Budget:
  index seconds: 45.657 -> 43.334 (limit x1.25)
  peak RSS KiB: 236700 -> 236036 (limit x1.25)
  export bytes: 28672668 -> 29136567 (limit x1.5)
Export, base -> head:
  calls: +0 -0 ~1629
    candidates: None -> [10 items]: 1
      github.com/claudiomascaro/meada::src/lang/mod.rs::known_outside::language.extensions#0
    candidates: None -> [14 items]: 31
      github.com/claudiomascaro/meada::src/declared.rs::Types.read_with::self.spent.of#0
      github.com/claudiomascaro/meada::src/document/yaml.rs::plain::text.parse#0
      github.com/claudiomascaro/meada::src/document/yaml.rs::plain::text.parse#1
      github.com/claudiomascaro/meada::src/jvm/spring.rs::inject::configs.of#0
      github.com/claudiomascaro/meada::src/lang/rust/cfg.rs::tests.manifest::text.parse#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::assignment::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::assignment::up.of#1
      github.com/claudiomascaro/meada::src/lang/ts.rs::binds::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::definition::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::definition::up.of#1
      github.com/claudiomascaro/meada::src/lang/ts.rs::in_options::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::in_options::up.of#1
      github.com/claudiomascaro/meada::src/lang/ts.rs::literal_owner::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts.rs::literal_owner::up.of#1
      github.com/claudiomascaro/meada::src/lang/ts.rs::literal_owner::up.of#2
      github.com/claudiomascaro/meada::src/lang/ts.rs::literal_owner::up.of#3
      github.com/claudiomascaro/meada::src/lang/ts.rs::literal_owner::up.of#4
      github.com/claudiomascaro/meada::src/lang/ts.rs::written_in_place::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts/types.rs::reference::up.of#0
      github.com/claudiomascaro/meada::src/lang/ts/uses.rs::Uses.enclosing::self.up.of#0
    candidates: None -> [2 items]: 377
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::authorize_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::compose_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::teams::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::answers::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#1
      github.com/claudiomascaro/meada::meada-core/src/named.rs::readable::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::visible::grant.reads#0
      github.com/claudiomascaro/meada::src/channel.rs::strongest::found.get#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::(call_expression).get#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::(call_expression).get#1
      github.com/claudiomascaro/meada::src/channel/go.rs::callback::call.args.get#0
      github.com/claudiomascaro/meada::src/channel/go.rs::claim_handler::call.args.get#0
      github.com/claudiomascaro/meada::src/channel/go.rs::configured::code.constant#0
      github.com/claudiomascaro/meada::src/channel/java.rs::of::call.args.get#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::call_of::code.constant#0
      github.com/claudiomascaro/meada::src/channel/ts.rs::messages::code.expand#0
      github.com/claudiomascaro/meada::src/channel/ts.rs::messages::code.expand#1
      github.com/claudiomascaro/meada::src/channel/ts.rs::messages::code.expand#2
      github.com/claudiomascaro/meada::src/channel/ts.rs::messages::code.expand#3
    candidates: None -> [20 items]: 3
      github.com/claudiomascaro/meada::src/package.rs::Package.written::self.ecosystem.name#0
      github.com/claudiomascaro/meada::src/package.rs::Providers.matching::ecosystem.name#0
      github.com/claudiomascaro/meada::src/package.rs::Providers.matching::ecosystem.name#1
    candidates: None -> [24 items]: 7
      github.com/claudiomascaro/meada::src/http/ts/clients.rs::response_reads::types.read#0
      github.com/claudiomascaro/meada::src/literals.rs::Reader.typed::self.types.read#0
      github.com/claudiomascaro/meada::src/resolve.rs::Code.field::self.resolver.read#0
      github.com/claudiomascaro/meada::src/resolve.rs::Code.name::self.resolver.read#0
      github.com/claudiomascaro/meada::src/resolve.rs::Resolver.lexical_as::reading.read#0
      github.com/claudiomascaro/meada::src/resolve.rs::Resolver.lexical_as::reading.read#1
      github.com/claudiomascaro/meada::src/resolve.rs::Resolver.walk::reading.read#0
    candidates: None -> [3 items]: 175
      github.com/claudiomascaro/meada::meada-core/src/ids.rs::fence::text.parse#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.written#0
      github.com/claudiomascaro/meada::src/channel/go.rs::made_by::code.origins#0
      github.com/claudiomascaro/meada::src/channel/python.rs::imported::code.imported#0
      github.com/claudiomascaro/meada::src/channel/python.rs::receivers::code.origins#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::client::code.origins#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::client::code.origins#1
      github.com/claudiomascaro/meada::src/channel/ts.rs::factory::code.origins#0
      github.com/claudiomascaro/meada::src/community.rs::communities::nodes.extend#0
      github.com/claudiomascaro/meada::src/config.rs::read_own_up_to::root.join#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.scope::(call_expression).join#0
      github.com/claudiomascaro/meada::src/decide.rs::finish::engine.root#0
      github.com/claudiomascaro/meada::src/decide.rs::finish::engine.root#1
      github.com/claudiomascaro/meada::src/decide.rs::prune::engine.root#0
      github.com/claudiomascaro/meada::src/decide.rs::prune::engine.root#1
      github.com/claudiomascaro/meada::src/decide.rs::run::engine.root#0
    candidates: None -> [4 items]: 172
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.extract::inline.entry#0
      github.com/claudiomascaro/meada::src/channel/go.rs::claim_handler::code.method#0
      github.com/claudiomascaro/meada::src/channel/go.rs::configured::code.value#0
      github.com/claudiomascaro/meada::src/channel/java.rs::exchange_type::code.value#0
      github.com/claudiomascaro/meada::src/channel/java.rs::listen::code.value#0
      github.com/claudiomascaro/meada::src/channel/java.rs::listen::code.value#1
      github.com/claudiomascaro/meada::src/channel/java.rs::listen::code.value#2
      github.com/claudiomascaro/meada::src/community.rs::communities::count.entry#0
      github.com/claudiomascaro/meada::src/community.rs::communities::weights.entry#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::label.entry#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::number.entry#0
      github.com/claudiomascaro/meada::src/community.rs::local_moving::weight.entry#0
      github.com/claudiomascaro/meada::src/community.rs::refine::weight.entry#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.new::taken.entry#0
      github.com/claudiomascaro/meada::src/declared.rs::Reader.field::self.out.fields.entry#0
      github.com/claudiomascaro/meada::src/document/asyncapi.rs::read::payloads.entry#0
      github.com/claudiomascaro/meada::src/engine.rs::contracts::files.at#0
      github.com/claudiomascaro/meada::src/engine.rs::contracts::files.at#1
      github.com/claudiomascaro/meada::src/engine.rs::line::files.at#0
      github.com/claudiomascaro/meada::src/engine.rs::state_text::other.text#0
    candidates: None -> [5 items]: 75
      github.com/claudiomascaro/meada::src/channel/csharp.rs::callback::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#1
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#2
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#3
      github.com/claudiomascaro/meada::src/channel/go.rs::messages::code.call#0
      github.com/claudiomascaro/meada::src/channel/java.rs::listen::code.call#0
      github.com/claudiomascaro/meada::src/channel/python.rs::callback::code.call#0
      github.com/claudiomascaro/meada::src/channel/python.rs::head::code.call#0
      github.com/claudiomascaro/meada::src/channel/python.rs::receivers::code.call#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::client::code.call#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::grouped::code.call#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::grouped::code.call#1
      github.com/claudiomascaro/meada::src/channel/rust.rs::record::code.call#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::record::code.call#1
      github.com/claudiomascaro/meada::src/channel/rust.rs::sqs::code.call#0
      github.com/claudiomascaro/meada::src/declared.rs::Reader.walk::self.spent.charge#0
      github.com/claudiomascaro/meada::src/entry.rs::exports::local.names.def#0
      github.com/claudiomascaro/meada::src/entry.rs::programs::local.names.def#0
    candidates: None -> [6 items]: 1
      github.com/claudiomascaro/meada::src/package.rs::resolutions_by_ways::ecosystem.resolve#0
    candidates: None -> [8 items]: 14
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.receiver#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::messages::code.receiver#0
      github.com/claudiomascaro/meada::src/channel/rust.rs::sqs::code.receiver#0
      github.com/claudiomascaro/meada::src/http/csharp/clients.rs::is_client::code.receiver#0
      github.com/claudiomascaro/meada::src/http/go/clients.rs::Clients.hand::self.code.receiver#0
      github.com/claudiomascaro/meada::src/http/go/clients.rs::Clients.send::self.code.receiver#0
      github.com/claudiomascaro/meada::src/http/go/clients.rs::Clients.set::self.code.receiver#0
      github.com/claudiomascaro/meada::src/http/java/clients.rs::client::code.receiver#0
      github.com/claudiomascaro/meada::src/http/java/clients.rs::client::code.receiver#1
      github.com/claudiomascaro/meada::src/http/java/clients.rs::client::code.receiver#2
      github.com/claudiomascaro/meada::src/http/java/declarative.rs::builds::code.receiver#0
      github.com/claudiomascaro/meada::src/http/python/clients.rs::consumers::code.receiver#0
      github.com/claudiomascaro/meada::src/http/swift/client.rs::consumers::code.receiver#0
      github.com/claudiomascaro/meada::src/resolve.rs::Resolver.with_shared::language.receiver#0
    candidates: None -> [9 items]: 11
      github.com/claudiomascaro/meada::src/decider/mod.rs::setting_on_disk::pack.config_files#0
      github.com/claudiomascaro/meada::src/http/csharp/clients.rs::registered::code.reaches#0
      github.com/claudiomascaro/meada::src/http/csharp/minimal.rs::Minimal.framework::self.code.reaches#0
      github.com/claudiomascaro/meada::src/http/swift/vapor.rs::Vapor.framework::self.code.reaches#0
      github.com/claudiomascaro/meada::src/package.rs::holding::ecosystem.is_manifest#0
      github.com/claudiomascaro/meada::src/package.rs::reachable::ecosystem.entries#0
      github.com/claudiomascaro/meada::src/package.rs::reachable::ecosystem.reaches#0
      github.com/claudiomascaro/meada::src/package/maven.rs::declares::eco.entries#0
      github.com/claudiomascaro/meada::src/package/maven.rs::reachable::eco.reaches#0
      github.com/claudiomascaro/meada::src/resolve.rs::tests.exact_in::pack.modules#0
      github.com/claudiomascaro/meada::src/resolve.rs::tests.over::pack.modules#0
    level: None -> ambiguous: 523
      github.com/claudiomascaro/meada::meada-core/src/ids.rs::fence::text.parse#0
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.extract::inline.entry#0
      github.com/claudiomascaro/meada::src/channel.rs::strongest::found.get#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::(call_expression).get#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::(call_expression).get#1
      github.com/claudiomascaro/meada::src/channel/go.rs::callback::call.args.get#0
      github.com/claudiomascaro/meada::src/channel/go.rs::claim_handler::call.args.get#0
      github.com/claudiomascaro/meada::src/channel/java.rs::of::call.args.get#0
      github.com/claudiomascaro/meada::src/community.rs::communities::count.entry#0
      github.com/claudiomascaro/meada::src/community.rs::communities::nodes.extend#0
      github.com/claudiomascaro/meada::src/community.rs::communities::weights.entry#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::label.entry#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::number.entry#0
      github.com/claudiomascaro/meada::src/community.rs::local_moving::weight.entry#0
      github.com/claudiomascaro/meada::src/community.rs::local_moving::weight.get#0
      github.com/claudiomascaro/meada::src/community.rs::refine::weight.entry#0
      github.com/claudiomascaro/meada::src/config.rs::read_own_up_to::root.join#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.new::taken.entry#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.scope::(call_expression).join#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.scope::self.named.get#0
    level: None -> inferred: 472
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.declare::export.consumers.push#0
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.declare::export.providers.push#0
      github.com/claudiomascaro/meada::src/community.rs::communities::(call_expression).find#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::(call_expression).sum#0
      github.com/claudiomascaro/meada::src/config.rs::read_up_to::file.take#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.new::proposed.push#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::read::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::read::response.answers.keys#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::validated::(call_expression).sum#0
      github.com/claudiomascaro/meada::src/decider/mod.rs::setting_on_disk::found.push#0
      github.com/claudiomascaro/meada::src/decider/validate.rs::offered::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decisions.rs::load_up_to::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decisions.rs::replay::links.push#0
      github.com/claudiomascaro/meada::src/declared.rs::Reader.walk::self.visiting.push#0
      github.com/claudiomascaro/meada::src/document/schema.rs::Reader.reference::self.stack.push#0
      github.com/claudiomascaro/meada::src/document/yaml.rs::Builder.MarkedEventReceiver.on_event::self.stack.push#0
      github.com/claudiomascaro/meada::src/dotnet/msbuild.rs::chain::found.push#0
      github.com/claudiomascaro/meada::src/dotnet/msbuild.rs::read::layers.push#0
      github.com/claudiomascaro/meada::src/engine.rs::Engine.contract_sides::sides.push#0
      github.com/claudiomascaro/meada::src/engine.rs::Engine.search_communities::hits.push#0
    level: exact -> ambiguous: 344
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::authorize_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::compose_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::teams::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::answers::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#1
      github.com/claudiomascaro/meada::meada-core/src/named.rs::readable::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::visible::grant.reads#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::callback::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#1
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#2
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#3
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.receiver#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.written#0
    level: exact -> inferred: 290
      github.com/claudiomascaro/meada::meada-core/src/context.rs::shape_lines::shape.source.at#0
      github.com/claudiomascaro/meada::meada-core/src/shape.rs::Shape.graft::import.want.declares#0
      github.com/claudiomascaro/meada::meada-core/src/shape.rs::drift::provider.source.declared#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::attached::code.bounds#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::function::code.definition#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::held_by::code.bindings#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.files#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.pack#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.bounds#0
      github.com/claudiomascaro/meada::src/channel/go.rs::callback::code.definition#0
      github.com/claudiomascaro/meada::src/channel/go.rs::configured::code.field_written#0
      github.com/claudiomascaro/meada::src/channel/go.rs::messages::code.files#0
      github.com/claudiomascaro/meada::src/channel/go.rs::messages::code.pack#0
      github.com/claudiomascaro/meada::src/channel/java.rs::array::call.keyword#0
      github.com/claudiomascaro/meada::src/channel/java.rs::binding::code.files#0
      github.com/claudiomascaro/meada::src/channel/java.rs::binds::code.files#0
      github.com/claudiomascaro/meada::src/channel/java.rs::is_class::code.files#0
      github.com/claudiomascaro/meada::src/channel/java.rs::of::code.definition#0
      github.com/claudiomascaro/meada::src/channel/message.rs::bind_with::code.expand_with#0
      github.com/claudiomascaro/meada::src/channel/message.rs::consume_with::code.expand_with#0
    target: None -> set: 472
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.declare::export.consumers.push#0
      github.com/claudiomascaro/meada::src/channel.rs::Channel.ContractKind.declare::export.providers.push#0
      github.com/claudiomascaro/meada::src/community.rs::communities::(call_expression).find#0
      github.com/claudiomascaro/meada::src/community.rs::leiden::(call_expression).sum#0
      github.com/claudiomascaro/meada::src/config.rs::read_up_to::file.take#0
      github.com/claudiomascaro/meada::src/contract.rs::Names.new::proposed.push#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::read::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::read::response.answers.keys#0
      github.com/claudiomascaro/meada::src/decider/decisions.rs::validated::(call_expression).sum#0
      github.com/claudiomascaro/meada::src/decider/mod.rs::setting_on_disk::found.push#0
      github.com/claudiomascaro/meada::src/decider/validate.rs::offered::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decisions.rs::load_up_to::(call_expression).find#0
      github.com/claudiomascaro/meada::src/decisions.rs::replay::links.push#0
      github.com/claudiomascaro/meada::src/declared.rs::Reader.walk::self.visiting.push#0
      github.com/claudiomascaro/meada::src/document/schema.rs::Reader.reference::self.stack.push#0
      github.com/claudiomascaro/meada::src/document/yaml.rs::Builder.MarkedEventReceiver.on_event::self.stack.push#0
      github.com/claudiomascaro/meada::src/dotnet/msbuild.rs::chain::found.push#0
      github.com/claudiomascaro/meada::src/dotnet/msbuild.rs::read::layers.push#0
      github.com/claudiomascaro/meada::src/engine.rs::Engine.contract_sides::sides.push#0
      github.com/claudiomascaro/meada::src/engine.rs::Engine.search_communities::hits.push#0
    target: set -> None: 344
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::authorize_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::compose_entries::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/authorize.rs::teams::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::answers::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::creation::grant.reads#1
      github.com/claudiomascaro/meada::meada-core/src/named.rs::readable::grant.reads#0
      github.com/claudiomascaro/meada::meada-core/src/named.rs::visible::grant.reads#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::callback::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::client::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::delivery::code.written#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#1
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#2
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.call#3
      github.com/claudiomascaro/meada::src/channel/csharp.rs::messages::code.receiver#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.local#0
      github.com/claudiomascaro/meada::src/channel/csharp.rs::sender::code.written#0
  providers: +0 -4 ~0
    - http::GET::/admin/dashboard | github.com/claudiomascaro/meada::tests/fixtures/conformance-rust/crates/actix/src/admin.rs::dashboard
    - http::GET::/more/extras | github.com/claudiomascaro/meada::tests/fixtures/conformance-rust/crates/actix/src/admin.rs::extra
    - http::GET::/status | github.com/claudiomascaro/meada::tests/fixtures/conformance-rust/crates/actix/src/admin.rs::status
    - http::GET::/tools/audit | github.com/claudiomascaro/meada::tests/fixtures/conformance-rust/crates/actix/src/admin.rs::audit
  communities: +52 -45
  processes: +25 -29
Confirm in the source (10 of 762 new or changed resolutions):
- call `github.com/claudiomascaro/meada::meada-core/src/context.rs::shape_lines::shape.source.at#0` [inferred] (was exact)
  site meada-core/src/context.rs:139: `let at = shape.source.at();`
  target `github.com/claudiomascaro/meada::meada-core/src/shape.rs::Source.at` at meada-core/src/shape.rs:346: `pub fn at(&self) -> String {`
- call `github.com/claudiomascaro/meada::src/engine.rs::Engine.search_communities::hits.push#0` [inferred] (was None)
  site src/engine.rs:667: `hits.push((r, format!("{repository} {name}"), line));`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
- call `github.com/claudiomascaro/meada::src/http/java/declarative.rs::Base.of::client.keyword#3` [inferred] (was exact)
  site src/http/java/declarative.rs:199: `let name = (client.keyword("name").or(client.keyword("value")))`
  target `github.com/claudiomascaro/meada::src/lang/mod.rs::Call.keyword` at src/lang/mod.rs:1072: `pub fn keyword(&self, name: &str) -> Option<&Value> {`
- call `github.com/claudiomascaro/meada::src/http/ts.rs::express::code.args#1` [inferred] (was exact)
  site src/http/ts.rs:213: `let Some([first, .., handler]) = code.args(path, n) else {`
  target `github.com/claudiomascaro/meada::src/resolve.rs::Code.args` at src/resolve.rs:5118: `pub fn args(&self, path: &'a str, index: usize) -> Option<&'a [Value]> {`
- call `github.com/claudiomascaro/meada::src/lang/csharp.rs::wildcard::facts.imports.push#0` [inferred] (was None)
  site src/lang/csharp.rs:360: `facts.imports.push(Import {`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
- call `github.com/claudiomascaro/meada::src/lang/kotlin.rs::template::parts.push#0` [inferred] (was None)
  site src/lang/kotlin.rs:1765: `parts.push(Part::Ref(name[..end].into()));`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
- call `github.com/claudiomascaro/meada::src/lang/rust.rs::parse::paths.push#3` [inferred] (was None)
  site src/lang/rust.rs:642: `paths.push((Slot::Named(facts.receivers.len()), path));`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
- call `github.com/claudiomascaro/meada::src/lang/swift/manifest.rs::pattern::node.kind#0` [inferred] (was None)
  site src/lang/swift/manifest.rs:125: `if node.kind() != "line_string_literal" {`
  target `github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind` at src/http/rust.rs:207: `fn kind(&self, place: Place<'a>) -> Option<Made> {`
- call `github.com/claudiomascaro/meada::src/package/go.rs::Go.Ecosystem.read::dependencies.push#1` [inferred] (was None)
  site src/package/go.rs:90: `dependencies.push(Entry {`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
- call `github.com/claudiomascaro/meada::src/resolve.rs::csharp_tests.unbound_calls_against::sources.push#0` [inferred] (was None)
  site src/resolve.rs:10672: `sources.push(("App/C.cs".into(), app));`
  target `github.com/claudiomascaro/meada::src/trace.rs::Walk.push` at src/trace.rs:712: `fn push(&mut self, depth: usize, mut row: Row) {`
MCP context {"symbol": "github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind"}: differs
```
--- base
+++ head
@@ -63,2 +63,3 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:1445 lambdas [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1484 held [inferred: one method named kind]
 github.com/claudiomascaro/meada src/lang/csharp.rs:1517 using [inferred: one method named kind]
@@ -74,2 +75,3 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:1624 condition [inferred: one method named kind]
+github.com/claudiomascaro/meada src/lang/csharp.rs:1633 receiver_of [inferred: one method named kind]
 github.com/claudiomascaro/meada src/lang/csharp.rs:1644 attributes [inferred: one method named kind]
@@ -81,4 +83,2 @@
 github.com/claudiomascaro/meada src/lang/csharp.rs:1708 base_class [inferred: one method named kind]
-github.com/claudiomascaro/meada src/lang/csharp.rs:1709 base_class [inferred: one method named kind]
-github.com/claudiomascaro/meada src/lang/csharp.rs:1740 type_spelling [inferred: one method named kind]
-… cut after 83 of 685 lines (budget 2000 tokens); raise budget for more
+… cut after 83 of 766 lines (budget 2000 tokens); raise budget for more
```
MCP trace {"from": "github.com/claudiomascaro/meada::src/http/rust.rs::Web.kind"}: differs
```
--- base
+++ head
@@ -2,3 +2,3 @@
   github.com/claudiomascaro/meada src/http/rust.rs:198 Web.made
-    github.com/claudiomascaro/meada src/resolve.rs:5113 Code.files
+    github.com/claudiomascaro/meada src/resolve.rs:5113 Code.files [inferred: one method named files]
     github.com/claudiomascaro/meada src/http/rust.rs:51 made_of
@@ -6,70 +6,65 @@
         github.com/claudiomascaro/meada src/trace.rs:198 Graph.find [inferred: one method named find]
-          github.com/claudiomascaro/meada src/contract.rs:360 kind_of
-            github.com/claudiomascaro/meada src/trace.rs:198 Graph.find [inferred: one method named find] (cycle)
-            github.com/claudiomascaro/meada src/channel.rs:112 Channel.ContractKind.prefix [ambiguous: 6 methods named prefix]
-            github.com/claudiomascaro/meada src/contract.rs:174 ContractKind.prefix [ambiguous: 6 methods named prefix]
-            github.com/claudiomascaro/meada src/http.rs:562 Http.ContractKind.prefix [ambiguous: 6 methods named prefix]
-            github.com/claudiomascaro/meada src/package.rs:111 Ecosystem.prefix [ambiguous: 6 methods named prefix]
-            github.com/claudiomascaro/meada src/package.rs:1696 Packages.ContractKind.prefix [ambiguous: 6 methods named prefix]
-            github.com/claudiomascaro/meada src/resource.rs:80 Resource.ContractKind.prefix [ambiguous: 6 methods named prefix]
-          github.com/claudiomascaro/meada src/channel.rs:116 Channel.ContractKind.sides [ambiguous: 4 methods named sides]
-          github.com/claudiomascaro/meada src/contract.rs:272 ContractKind.sides [ambiguous: 4 methods named sides]
-          github.com/claudiomascaro/meada src/package.rs:1700 Packages.ContractKind.sides [ambiguous: 4 methods named sides]
-          github.com/claudiomascaro/meada src/resource.rs:84 Resource.ContractKind.sides [ambiguous: 4 methods named sides]
-          github.com/claudiomascaro/meada src/contract.rs:329 ContractKind.starts [ambiguous: 2 methods named starts]
-          github.com/claudiomascaro/meada src/http.rs:828 Http.ContractKind.starts [ambiguous: 2 methods named starts]
-            github.com/claudiomascaro/meada src/http.rs:175 route
-            github.com/claudiomascaro/meada src/http.rs:33 contract
-            github.com/claudiomascaro/meada src/http.rs:188 fits
-          github.com/claudiomascaro/meada src/channel.rs:379 fits [ambiguous: 3 functions or classes named fits]
-            github.com/claudiomascaro/meada src/channel.rs:103 parse
-            github.com/claudiomascaro/meada src/channel.rs:448 wildcard
-              github.com/claudiomascaro/meada src/channel.rs:458 nats
-            github.com/claudiomascaro/meada src/package.rs:1621 reason [inferred: one function or class named reason]
-              github.com/claudiomascaro/meada src/package.rs:1614 flag_of
-                github.com/claudiomascaro/meada src/package.rs:135 Ecosystem.flag
-                github.com/claudiomascaro/meada src/package.rs:372 of_contract
+          github.com/claudiomascaro/meada src/dotnet/appsettings.rs:55 Settings.get [ambiguous: 2 methods named get]
+            github.com/claudiomascaro/meada src/dotnet/appsettings.rs:55 Settings.get [ambiguous: 2 methods named get] (cycle)
+            github.com/claudiomascaro/meada src/render.rs:25 Files.get [ambiguous: 2 methods named get]
+              github.com/claudiomascaro/meada src/channel.rs:356 Channel.ContractKind.entry [ambiguous: 4 methods named entry]
+                github.com/claudiomascaro/meada src/channel.rs:103 parse
+              github.com/claudiomascaro/meada src/contract.rs:185 ContractKind.entry [ambiguous: 4 methods named entry]
+              github.com/claudiomascaro/meada src/http.rs:851 Http.ContractKind.entry [ambiguous: 4 methods named entry]
+                github.com/claudiomascaro/meada src/http.rs:175 route
+              github.com/claudiomascaro/meada src/package.rs:958 Provided.entry [ambiguous: 4 methods named entry]
+                github.com/claudiomascaro/meada src/trace.rs:198 Graph.find [inferred: one method named find] (cycle)
+                github.com/claudiomascaro/meada src/package.rs:991 Provided.owned_source
+                  github.com/claudiomascaro/meada src/package.rs:328 of
+                    github.com/claudiomascaro/meada src/trace.rs:198 Graph.find [inferred: one method named find] (cycle)
+                    github.com/claudiomascaro/meada src/package.rs:79 Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package.rs:2420 tests.Stand.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package/cargo.rs:137 Cargo.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package/go.rs:46 Go.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package/maven.rs:518 Maven.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                      github.com/claudiomascaro/meada src/package/maven.rs:388 declarations
+                    github.com/claudiomascaro/meada src/package/npm.rs:54 Npm.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package/nuget.rs:136 Nuget.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                    github.com/claudiomascaro/meada src/package/pypi.rs:233 Pypi.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                      github.com/claudiomascaro/meada src/package/pypi.rs:32 is_requirements
+                    github.com/claudiomascaro/meada src/package/swiftpm.rs:329 Swiftpm.Ecosystem.is_manifest [ambiguous: 9 methods named is_manifest]
+                  github.com/claudiomascaro/meada src/package.rs:952 Provided.source
+                  github.com/claudiomascaro/meada src/package.rs:786 owner
```
MCP context {"symbol": "github.com/claudiomascaro/meada::src/trace.rs::Walk.push"}: differs
```
--- base
+++ head
@@ -5,2 +5,4 @@
 github.com/claudiomascaro/meada src/channel.rs:204 Channel.ContractKind.extract [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel.rs:286 Channel.ContractKind.declare [inferred: one method named push]
+github.com/claudiomascaro/meada src/channel.rs:292 Channel.ContractKind.declare [inferred: one method named push]
 github.com/claudiomascaro/meada src/channel/csharp.rs:602 tests.lines_with [inferred: one method named push]
@@ -14,3 +16,5 @@
 github.com/claudiomascaro/meada src/community.rs:143 leiden [inferred: one method named push]
+github.com/claudiomascaro/meada src/contract.rs:452 Names.new [inferred: one method named push]
 github.com/claudiomascaro/meada src/decider/mod.rs:256 setting_on_disk [inferred: one method named push]
+github.com/claudiomascaro/meada src/decider/mod.rs:264 setting_on_disk [inferred: one method named push]
 github.com/claudiomascaro/meada src/decider/mod.rs:481 run [inferred: one method named push]
@@ -18,12 +22,29 @@
 github.com/claudiomascaro/meada src/decisions.rs:346 replay [inferred: one method named push]
+github.com/claudiomascaro/meada src/decisions.rs:353 replay [inferred: one method named push]
+github.com/claudiomascaro/meada src/declared.rs:168 Reader.walk [inferred: one method named push]
 github.com/claudiomascaro/meada src/document/asyncapi.rs:72 read [inferred: one method named push]
+github.com/claudiomascaro/meada src/document/schema.rs:232 Reader.reference [inferred: one method named push]
 github.com/claudiomascaro/meada src/document/yaml.rs:113 Builder.place [inferred: one method named push]
+github.com/claudiomascaro/meada src/document/yaml.rs:169 Builder.MarkedEventReceiver.on_event [inferred: one method named push]
+github.com/claudiomascaro/meada src/dotnet/msbuild.rs:153 read [inferred: one method named push]
+github.com/claudiomascaro/meada src/dotnet/msbuild.rs:223 chain [inferred: one method named push]
 github.com/claudiomascaro/meada src/dotnet/msbuild.rs:314 layer [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:363 Engine.status_report [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:368 Engine.status_report [inferred: one method named push]
 github.com/claudiomascaro/meada src/engine.rs:545 Engine.search [inferred: one method named push]
 github.com/claudiomascaro/meada src/engine.rs:602 Engine.search [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:633 Engine.search_documents [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:667 Engine.search_communities [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:687 Engine.search_processes [inferred: one method named push]
 github.com/claudiomascaro/meada src/engine.rs:727 Engine.search_duplicates [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:790 Engine.search_items [inferred: one method named push]
+github.com/claudiomascaro/meada src/engine.rs:845 Engine.contract_sides [inferred: one method named push]
 github.com/claudiomascaro/meada src/engine.rs:854 Engine.contract_sides [inferred: one method named push]
+github.com/claudiomascaro/meada src/entry.rs:219 keyed_defs [inferred: one method named push]
 github.com/claudiomascaro/meada src/entry.rs:270 tests.found [inferred: one method named push]
 github.com/claudiomascaro/meada src/entry.rs:302 tests.export_doors [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:209 drift [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:218 drift [inferred: one method named push]
 github.com/claudiomascaro/meada src/http.rs:344 based [inferred: one method named push]
+github.com/claudiomascaro/meada src/http.rs:505 prefixes_within [inferred: one method named push]
 github.com/claudiomascaro/meada src/http.rs:648 Http.ContractKind.declare [inferred: one method named push]
@@ -34,4 +55,7 @@
 github.com/claudiomascaro/meada src/http/csharp/clients.rs:412 Index.of [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/clients.rs:417 Index.of [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/csharp/clients.rs:442 registration [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/csharp/clients.rs:638 tests.sent_in [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/minimal.rs:119 Minimal.intern [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/csharp/minimal.rs:120 Minimal.intern [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/csharp/refit.rs:85 consumers [inferred: one method named push]
@@ -40,5 +64,12 @@
 github.com/claudiomascaro/meada src/http/csharp/refit.rs:228 url [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go.rs:416 Go.reach [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go.rs:579 Go.chains [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/go.rs:585 Go.chains [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go.rs:765 Go.provide [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/go.rs:795 providers [inferred: one method named push]
 github.com/claudiomascaro/meada src/http/go/clients.rs:176 Clients.reaches [inferred: one method named push]
+github.com/claudiomascaro/meada src/http/go/clients.rs:219 Clients.traced [inferred: one method named push]
```
MCP trace {"from": "github.com/claudiomascaro/meada::src/trace.rs::Walk.push"}: differs
```
--- base
+++ head
@@ -1,2 +1,3 @@
 github.com/claudiomascaro/meada src/trace.rs:712 Walk.push
-chain: none · 0 hops · 1 repos · 27 tokens
+  github.com/claudiomascaro/meada src/trace.rs:712 Walk.push [inferred: one method named push] (cycle)
+chain: inferred · 0 hops · 1 repos · 53 tokens
```
MCP stdio: 7/11 answers byte-equal; queries in /home/user/meada/.prove-it/runs/20261103T045241Z-audit-M5/queries/meada.jsonl

## MCP HTTP vs stdio (head)

  meada: tools same, 11/11 answers byte-equal to stdio

