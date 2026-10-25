//! The language conformance suite (ADR 0007): a language pack is done when the export of its
//! conformance fixture holds exactly the expected definitions, every expected call at its
//! expected level, and matches its golden file. A new pack adds a fixture under
//! `tests/fixtures/conformance-<language>/` and one test calling `conform`.

use crate::common;

use common::{assert_golden, fixture_repo};
use meada::engine::Engine;
use serde_json::Value;

const ORIGIN: &str = "https://example.com/acme/conformance.git";
const REPO: &str = "example.com/acme/conformance";

/// An expected call site: its id and level, and its targets (one for `exact` and `inferred`,
/// every candidate for `ambiguous`, none when the call stays unresolved). Ids are relative to
/// the repository.
type ExpectedCall<'a> = (&'a str, Option<&'a str>, &'a [&'a str]);

const EXACT: Option<&str> = Some("exact");
const INFERRED: Option<&str> = Some("inferred");
const AMBIGUOUS: Option<&str> = Some("ambiguous");

#[test]
fn java() {
    conform(
        "conformance-java",
        &[
            "maven/api/src/main/java/example/Orders.java::example.Orders",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Orders()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.main(String[])",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Orders(String)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.place(String,int)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.place(String)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.place(int)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.start()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.imported()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.enclosed()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.qualified(java.lang.String)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.shadow(Object)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.localShadow()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.check()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.hidden()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Nested",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Nested.run()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Protected",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Protected.run()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Private",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Private.run()",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Package",
            "maven/api/src/main/java/example/Orders.java::example.Orders.Package.run()",
            "maven/api/src/main/java/example/Local.java::example.Local",
            "maven/api/src/main/java/example/Local.java::example.Local.same()",
            "maven/api/src/main/java/example/util/Tools.java::example.util.Tools",
            "maven/api/src/main/java/example/util/Tools.java::example.util.Tools.single()",
            "maven/api/src/main/java/example/util/Tools.java::example.util.Tools.doubled()",
            "maven/api/src/main/java/example/util/Tools.java::example.util.Tools.valueOf(int)",
            "maven/api/src/main/java/example/stars/Wave.java::example.stars.Wave",
            "maven/api/src/main/java/example/stars/Wave.java::example.stars.Wave.wave()",
            "gradle/src/main/java/example/Receipt.java::example.Receipt",
            "gradle/src/main/java/example/Receipt.java::example.Receipt.Receipt(String)",
            "gradle/src/main/java/example/Receipt.java::example.Pending",
            "gradle/src/main/java/example/Receipt.java::example.Pending.finish()",
            "gradle/src/main/java/example/Receipt.java::example.State",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.touch(int)",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.mark(int)",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.ping()",
            "maven/api/src/main/java/example/calls/Port.java::example.calls.Port",
            "maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String)",
            "maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String,int)",
            "maven/api/src/main/java/example/calls/Noise.java::example.calls.Noise",
            "maven/api/src/main/java/example/calls/Noise.java::example.calls.Noise.ping()",
            "maven/api/src/main/java/example/calls/Noise.java::example.calls.Noise.flush()",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.save()",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.touch(int)",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.mark(String)",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.send(String)",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.send(String,int)",
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()",
            "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing",
            "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.own()",
            "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.use()",
            "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.Task",
            "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.Task.run()",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.make()",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.chained()",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.shadowed(Port)",
            "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.unknown()",
            "maven/api/src/main/java/example/Mains.java::example.Mains",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Spread",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Spread.main(String...)",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Instance",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Instance.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Hidden",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Hidden.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Wrong",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Wrong.main(String)",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Wrong.main()",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Wrong.main(int[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Iface",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Iface.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Closed",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Closed.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Decorated",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Decorated.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Qualified",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Qualified.main(java.lang.String[])",
            "maven/api/src/main/java/example/clients/Injected.java::example.clients.Injected",
            "maven/api/src/main/java/example/clients/Injected.java::example.clients.Injected.Injected(RestTemplate)",
            "maven/api/src/main/java/example/clients/Injected.java::example.clients.Injected.send()",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.Rest(RestTemplate)",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.list()",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.one(String)",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.create()",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.flag(int)",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.single()",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.send(HttpMethod,String)",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.remove(String)",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.drop()",
            "maven/api/src/main/java/example/clients/Rest.java::example.clients.Rest.lookup()",
            "maven/api/src/main/java/example/clients/Reactive.java::example.clients.Reactive",
            "maven/api/src/main/java/example/clients/Reactive.java::example.clients.Reactive.get()",
            "maven/api/src/main/java/example/clients/Reactive.java::example.clients.Reactive.flag()",
            "maven/api/src/main/java/example/clients/Jdk.java::example.clients.Jdk",
            "maven/api/src/main/java/example/clients/Jdk.java::example.clients.Jdk.list()",
            "maven/api/src/main/java/example/clients/Jdk.java::example.clients.Jdk.create()",
            "maven/api/src/main/java/example/clients/Jdk.java::example.clients.Jdk.replace()",
            "maven/api/src/main/java/example/clients/Ok.java::example.clients.Ok",
            "maven/api/src/main/java/example/clients/Ok.java::example.clients.Ok.list()",
            "maven/api/src/main/java/example/clients/Ok.java::example.clients.Ok.remove()",
            "maven/api/src/main/java/example/clients/OwnEnum.java::example.clients.OwnEnum",
            "maven/api/src/main/java/example/clients/OwnEnum.java::example.clients.OwnEnum.HttpMethod",
            "maven/api/src/main/java/example/clients/OwnEnum.java::example.clients.OwnEnum.get()",
            "maven/api/src/main/java/example/web/Api.java::example.web.Api",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.list()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.get(String)",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.create()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.replace()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.flag()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.remove()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.several()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.single()",
            "maven/api/src/main/java/example/web/OrderController.java::example.web.OrderController.any()",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController.page()",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController.store()",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController.nested()",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController.Inner",
            "maven/api/src/main/java/example/web/PageController.java::example.web.PageController.Inner.echo()",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Plain",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Plain.mapped()",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Unreadable",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Unreadable.known()",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Unreadable.moving()",
            "maven/api/src/main/java/example/web/Plain.java::example.web.Unreadable.partial()",
            "maven/api/src/main/java/example/web/Wildcard.java::example.web.Wildcard",
            "maven/api/src/main/java/example/web/Wildcard.java::example.web.Wildcard.card()",
            "maven/api/src/main/java/example/web/Wildcard.java::example.web.Wildcard.removed()",
            "maven/api/src/main/java/example/web/shadow/GetMapping.java::example.web.shadow.GetMapping",
            "maven/api/src/main/java/example/web/shadow/RequestMethod.java::example.web.shadow.RequestMethod",
            "maven/api/src/main/java/example/web/shadow/Wild.java::example.web.shadow.Wild",
            "maven/api/src/main/java/example/web/shadow/Wild.java::example.web.shadow.Wild.anno()",
            "maven/api/src/main/java/example/web/shadow/Wild.java::example.web.shadow.Wild.local()",
            "maven/api/src/main/java/example/web/shadow/Wild.java::example.web.shadow.Wild.ok()",
            "maven/api/src/main/java/example/web/shadow/Shadowed.java::example.web.shadow.Shadowed",
            "maven/api/src/main/java/example/web/shadow/Shadowed.java::example.web.shadow.Shadowed.ok()",
            "maven/api/src/main/java/example/web/shadow/Shadowed.java::example.web.shadow.Shadowed.local()",
            "maven/api/src/main/java/example/web/shadow/Shadowed.java::example.web.shadow.Shadowed.outside()",
            "maven/api/src/main/java/example/declarative/BillingClient.java::example.declarative.BillingClient",
            "maven/api/src/main/java/example/declarative/BillingClient.java::example.declarative.BillingClient.invoice(String)",
            "maven/api/src/main/java/example/declarative/BillingClient.java::example.declarative.BillingClient.purge()",
            "maven/api/src/main/java/example/declarative/Ledger.java::example.declarative.Ledger",
            "maven/api/src/main/java/example/declarative/Ledger.java::example.declarative.Ledger.entry(String)",
            "maven/api/src/main/java/example/declarative/Ledger.java::example.declarative.Ledger.record()",
            "maven/api/src/main/java/example/declarative/Remotes.java::example.declarative.Remotes",
            "maven/api/src/main/java/example/declarative/Remotes.java::example.declarative.Remotes.ledger()",
            "maven/api/src/main/java/example/declarative/OrdersClient.java::example.declarative.OrdersClient",
            "maven/api/src/main/java/example/declarative/OrdersClient.java::example.declarative.OrdersClient.find(String)",
            "maven/api/src/main/java/example/declarative/OrdersClient.java::example.declarative.OrdersClient.place(String)",
            "maven/api/src/main/java/example/declarative/Statements.java::example.declarative.Statements",
            "maven/api/src/main/java/example/declarative/Statements.java::example.declarative.Statements.Statements(OrdersClient,BillingClient)",
            "maven/api/src/main/java/example/declarative/Statements.java::example.declarative.Statements.report(String)",
            "maven/api/src/main/java/example/declarative/shadow/Contract.java::example.declarative.shadow.Contract",
            "maven/api/src/main/java/example/declarative/shadow/Contract.java::example.declarative.shadow.Contract.line()",
            "maven/api/src/main/java/example/declarative/shadow/GetMapping.java::example.declarative.shadow.GetMapping",
            "maven/api/src/main/java/example/declarative/shadow/Hidden.java::example.declarative.shadow.Hidden",
            "maven/api/src/main/java/example/declarative/shadow/Hidden.java::example.declarative.shadow.Hidden.hidden()",
            "maven/api/src/main/java/example/web/shadow/Shadowed.java::example.web.shadow.Shadowed.anno()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.list()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.get(String)",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.create()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.replace()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.flag()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.remove()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.probe()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.options()",
            "maven/api/src/main/java/example/web/jaxrs/Items.java::example.web.jaxrs.Items.locator()",
            "maven/api/src/main/java/example/web/jaxrs/Remote.java::example.web.jaxrs.Remote",
            "maven/api/src/main/java/example/web/jaxrs/Remote.java::example.web.jaxrs.Remote.fetch()",
            "maven/api/src/main/java/example/web/jaxrs/Jakarta.java::example.web.jaxrs.Jakarta",
            "maven/api/src/main/java/example/web/jaxrs/Jakarta.java::example.web.jaxrs.Jakarta.get()",
            "maven/api/src/main/java/example/web/jaxrs/Jakarta.java::example.web.jaxrs.Jakarta.create()",
            "maven/api/src/main/java/example/web/jaxrs/App.java::example.web.jaxrs.App",
            "maven/api/src/main/java/example/web/jaxrs/App.java::example.web.jaxrs.Ping",
            "maven/api/src/main/java/example/web/jaxrs/App.java::example.web.jaxrs.Ping.alive()",
            "maven/api/src/main/java/example/web/jaxrs/shadow/GET.java::example.web.jaxrs.shadow.GET",
            "maven/api/src/main/java/example/web/jaxrs/shadow/Hidden.java::example.web.jaxrs.shadow.Hidden",
            "maven/api/src/main/java/example/web/jaxrs/shadow/Hidden.java::example.web.jaxrs.shadow.Hidden.hidden()",
            "maven/api/src/main/java/example/web/jaxrs/shadow/Hidden.java::example.web.jaxrs.shadow.Hidden.visible()",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.Sales(KafkaTemplate<String,String>)",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.created(String)",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.paid(String)",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.dynamic(String)",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.onSale(String)",
            "maven/api/src/main/java/example/channels/Sales.java::example.channels.Sales.onMissing(String)",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.Billing(RabbitTemplate)",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.invoice(String)",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.refund(String)",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.audit()",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.onInvoice(String)",
            "maven/api/src/main/java/example/channels/Billing.java::example.channels.Billing.onRefund(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.Delivery(JmsTemplate,SqsClient,Connection)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.notifyCustomer(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.onNotification(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.ship(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.receive(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.shipments()",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.loop(Object)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.track(String)",
            "maven/api/src/main/java/example/channels/Delivery.java::example.channels.Delivery.watch(Dispatcher)",
            "maven/api/src/main/java/example/channels/Streams.java::example.channels.Streams",
            "maven/api/src/main/java/example/channels/Streams.java::example.channels.Streams.Streams(JedisPooled,RedisCommands<String,String>)",
            "maven/api/src/main/java/example/channels/Streams.java::example.channels.Streams.record(String)",
            "maven/api/src/main/java/example/channels/Streams.java::example.channels.Streams.consume()",
        ],
        &[
            "maven/api/src/main/java/example/Mains.java::example.Mains.Decorated.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Iface.main(String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Qualified.main(java.lang.String[])",
            "maven/api/src/main/java/example/Mains.java::example.Mains.Spread.main(String...)",
            "maven/api/src/main/java/example/Orders.java::example.Orders.main(String[])",
        ],
        JAVA_CALLS,
    );
}

/// Calls through typed receivers, written supertypes and name matches (#85).
const JAVA_CALLS: &[ExpectedCall] = &[
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.touch(int)::super.touch#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.touch(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.chained()::(method_invocation).save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::this.save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::this.save#1",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.save()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::touch#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.touch(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::mark#0",
        AMBIGUOUS,
        &[
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.mark(String)",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.mark(int)",
        ],
    ),
    (
        "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.run()::item.ping#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.ping()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.use()::own#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.own()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.use()::this.own#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.own()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.use()::this.send#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.Task.run()::own#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Listing.java::example.calls.Listing.own()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::repo.save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.save()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::repo.save#1",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::repo.ping#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.ping()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::this.repo.touch#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.touch(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::given.touch#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.touch(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::local.mark#0",
        AMBIGUOUS,
        &[
            "maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.mark(String)",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.mark(int)",
        ],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::fresh.save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.save()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::made.save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.save(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::qualified.save#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Repo.java::example.calls.Repo.save()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::port.send#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.typed(Repo)::port.send#1",
        EXACT,
        &["maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String,int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.shadowed(Port)::repo.send#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Port.java::example.calls.Port.send(String)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.shadowed(Port)::Repo.mark#0",
        EXACT,
        &["maven/api/src/main/java/example/calls/Store.java::example.calls.Store.mark(int)"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.unknown()::item.flush#0",
        INFERRED,
        &["maven/api/src/main/java/example/calls/Noise.java::example.calls.Noise.flush()"],
    ),
    (
        "maven/api/src/main/java/example/calls/Client.java::example.calls.Client.unknown()::item.ping#0",
        AMBIGUOUS,
        &[
            "maven/api/src/main/java/example/calls/Noise.java::example.calls.Noise.ping()",
            "maven/api/src/main/java/example/calls/Store.java::example.calls.Store.ping()",
        ],
    ),
];

#[test]
fn java_context_shows_a_declarative_client_method_as_a_consumer_its_caller_reaches() {
    let repo = fixture_repo("java_declarative_context", "conformance-java", Some(ORIGIN));
    assert_golden(
        "conformance-java.context-feign.txt",
        &Engine::open(&repo)
            .unwrap()
            .context("example.declarative.BillingClient.invoice(String)", None)
            .unwrap(),
    );
}

#[test]
fn java_context_shows_an_unresolved_consumer() {
    let repo = fixture_repo("java_unresolved_consumer", "conformance-java", Some(ORIGIN));
    assert_golden(
        "conformance-java.context-unresolved.txt",
        &Engine::open(&repo)
            .unwrap()
            .context("example.clients.Rest.lookup()", None)
            .unwrap(),
    );
}

/// Each Java channel client links its producer to its handler (spec #26, #92): a topic from the
/// module's Spring configuration, a publish to a named exchange routed by an annotation's and a
/// configuration's bindings, the default exchange, a NATS wildcard as `inferred`, and a name
/// that cannot be evaluated left unresolved on both sides.
#[test]
fn java_channel_clients_link_their_producers_to_their_handlers() {
    let repo = fixture_repo("java_channels", "conformance-java", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let short = |site: &str| {
        let site = site.rsplit("example/channels/").next().unwrap_or(site);
        site.split_once("::example.channels.")
            .map_or(site, |(_, s)| s)
            .to_string()
    };
    let links: Vec<String> = (json["links"].as_array().unwrap().iter())
        .filter(|l| l["contract"].as_str().unwrap().starts_with("channel::"))
        .map(|l| {
            let (level, contract) = (
                l["level"].as_str().unwrap(),
                l["contract"].as_str().unwrap(),
            );
            let (provider, consumer) = (
                l["provider"].as_str().unwrap(),
                l["consumer"].as_str().unwrap(),
            );
            format!(
                "{level} {contract} {} -> {}",
                short(provider),
                short(consumer)
            )
        })
        .collect();
    for wanted in [
        "exact channel::kafka::orders.created Sales.created(String)::kafka.send#0 -> Sales.onSale(String)",
        "exact channel::kafka::orders.paid Sales.paid(String)::kafka.send#0 -> Sales.onSale(String)",
        "inferred channel::amqp::invoices Billing.invoice(String)::rabbit.convertAndSend#0 -> Billing.onInvoice(String)",
        "inferred channel::amqp::audit Billing.invoice(String)::rabbit.convertAndSend#0 -> Billing.onRefund(String)",
        "exact channel::amqp::refunds Billing.refund(String)::rabbit.convertAndSend#0 -> Billing.onRefund(String)",
        "exact channel::jms::notifications Delivery.notifyCustomer(String)::jms.convertAndSend#0 -> Delivery.onNotification(String)",
        "exact channel::sqs::shipments Delivery.ship(String)::sqs.sendMessage#0 -> Delivery.shipments()",
        "inferred channel::nats::delivery.*.tracked Delivery.track(String)::nats.publish#0 -> Delivery.watch(Dispatcher)",
        "exact channel::redis::events Streams.record(String)::jedis.xadd#0 -> Streams.consume()",
        "exact channel::redis::audits Streams.record(String)::lettuce.xadd#0 -> Streams.consume()",
    ] {
        assert!(
            links.contains(&wanted.to_string()),
            "{wanted} in {links:#?}"
        );
    }
    assert_eq!(links.len(), 10, "{links:#?}");
    let unresolved = |side: &str| -> Vec<String> {
        (json[side].as_array().unwrap().iter())
            .filter(|s| s["contract"] == "channel::kafka")
            .map(|s| short(s["site"].as_str().unwrap()))
            .collect()
    };
    assert_eq!(
        unresolved("providers"),
        ["Sales.dynamic(String)::kafka.send#0"]
    );
    assert_eq!(unresolved("consumers"), ["Sales.onMissing(String)"]);
}

#[test]
fn java_context_lists_callers_through_receivers_and_name_matches() {
    let repo = fixture_repo("java_receiver_context", "conformance-java", Some(ORIGIN));
    assert_golden(
        "conformance-java.context-ping.txt",
        &Engine::open(&repo)
            .unwrap()
            .context("example.calls.Store.ping()", None)
            .unwrap(),
    );
}

#[test]
fn java_static_imports_an_outside_base_may_shadow_are_name_matches() {
    let repo = fixture_repo("java_shadowed_static", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("Job.java"),
        "package other; import static example.util.Tools.single; \
         public class Job extends Remote { void run() { single(); } } \
         class Plain { void run() { single(); } }",
    )
    .unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = |caller: &str| {
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["caller"].as_str().unwrap().ends_with(caller))
            .unwrap()
            .clone()
    };
    let target = "example.util.Tools.single()";
    let plain = call("other.Plain.run()");
    assert_eq!(plain["level"], "exact", "{plain}");
    assert!(plain["target"].as_str().unwrap().ends_with(target));
    // `Remote` may declare `single()`, which would shadow the import.
    let shadowed = call("other.Job.run()");
    assert_eq!(shadowed["level"], "inferred", "{shadowed}");
    assert!(shadowed["target"].as_str().unwrap().ends_with(target));
}

#[test]
fn java_static_calls_resolve_overloads_by_argument_count() {
    let repo = fixture_repo("java_static_calls", "conformance-java", Some(ORIGIN));
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls: Vec<_> = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["caller"]
                .as_str()
                .is_some_and(|c| c.ends_with("example.Orders.start()"))
        })
        .collect();
    assert_eq!(calls.len(), 4);
    let call = |suffix: &str| {
        calls
            .iter()
            .find(|c| c["id"].as_str().unwrap().ends_with(suffix))
            .unwrap()
    };
    for suffix in ["::check#0", "::this.check#0"] {
        assert_eq!(call(suffix)["level"], "exact");
        assert!(
            call(suffix)["target"]
                .as_str()
                .unwrap()
                .ends_with("example.Orders.check()")
        );
    }
    assert_eq!(call("::Orders.place#0")["level"], "exact");
    assert!(
        call("::Orders.place#0")["target"]
            .as_str()
            .unwrap()
            .ends_with("place(String,int)")
    );
    assert_eq!(call("::Orders.place#1")["level"], "ambiguous");
    assert_eq!(
        call("::Orders.place#1")["candidates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn java_imports_constructors_and_enclosed_calls_reach_the_written_symbols() {
    let repo = fixture_repo("java_imports", "conformance-java", Some(ORIGIN));
    let engine = Engine::open(&repo).unwrap();
    let json: Value = serde_json::from_str(&engine.export().unwrap()).unwrap();
    let calls = json["calls"].as_array().unwrap();
    let call = |scope: &str, suffix: &str| {
        calls
            .iter()
            .find(|c| {
                c["caller"].as_str().unwrap().ends_with(scope)
                    && c["id"].as_str().unwrap().ends_with(suffix)
            })
            .unwrap()
    };
    for (suffix, target) in [
        ("::Tools.single#0", "example.util.Tools.single()"),
        ("::single#0", "example.util.Tools.single()"),
        ("::doubled#0", "example.util.Tools.doubled()"),
        ("::Wave.wave#0", "example.stars.Wave.wave()"),
        ("::Local.same#0", "example.Local.same()"),
        ("::Orders#0", "example.Orders.Orders()"),
        ("::Orders#1", "example.Orders.Orders(String)"),
        ("::Receipt#0", "example.Receipt.Receipt(String)"),
    ] {
        let found = call("example.Orders.imported()", suffix);
        assert_eq!(found["level"], "exact", "{suffix}: {found}");
        assert!(
            found["target"].as_str().unwrap().ends_with(target),
            "{suffix}: {found}"
        );
    }
    for suffix in ["::String.valueOf#0", "::System.currentTimeMillis#0"] {
        assert!(call("example.Orders.imported()", suffix)["target"].is_null());
    }
    assert_eq!(
        call("example.Orders.enclosed()", "::check#0")["level"],
        "exact"
    );
    assert_eq!(
        call("example.Orders.enclosed()", "::(anonymous).check#0")["level"],
        "inferred"
    );
    assert_eq!(
        call(
            "example.Orders.qualified(java.lang.String)",
            "::this.check#0"
        )["level"],
        "exact"
    );
    for scope in [
        "example.Orders.shadow(Object)",
        "example.Orders.localShadow()",
    ] {
        assert_ne!(call(scope, "::Tools.single#0")["level"], "exact");
    }
    let doors: Vec<_> = json["processes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["entry_point"].as_str().unwrap())
        .collect();
    assert!(
        doors
            .iter()
            .any(|d| d.ends_with("example.Orders.Protected.run()"))
    );
    assert!(
        doors
            .iter()
            .any(|d| d.ends_with("example.Orders.qualified(java.lang.String)"))
    );
    for hidden in [
        "example.Orders.Private.run()",
        "example.Orders.Package.run()",
        "example.Orders.hidden()",
        "example.Pending.finish()",
    ] {
        assert!(!doors.iter().any(|d| d.ends_with(hidden)), "{hidden}");
    }
    assert_golden(
        "conformance-java.search-orders.txt",
        &engine.search("Orders", Some("symbol"), None).unwrap(),
    );
    assert_golden(
        "conformance-java.context-check.txt",
        &engine.context("example.Orders.check()", None).unwrap(),
    );
}

#[test]
fn java_hostile_definition_value_and_member_depths_remain_bounded() {
    let repo = fixture_repo("java_depths", "conformance-java", Some(ORIGIN));
    let mut deep = String::from("package hostile; public class Deep { void ping() {}");
    for n in 0..132 {
        deep.push_str(&format!("public class Inner{n} {{"));
    }
    deep.push_str("void ping() {} void leaf() { ping(); this.ping(); missing(); }");
    deep.push_str(&"}".repeat(133));
    std::fs::write(repo.join("Deep.java"), deep).unwrap();
    let expression = format!("{}\"value\"{}", "(".repeat(1024), ")".repeat(1024));
    let member = format!("root{}.send()", ".part".repeat(300));
    std::fs::write(repo.join("Values.java"), format!("package hostile; public class Values {{ static final String V = {expression}; public void run() {{ {member}; }} }}")).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let definitions = json["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["span"]["path"] == "Deep.java")
        .count();
    assert_eq!(definitions, 129);
    assert!(
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["span"]["path"] == "Deep.java")
            .all(|c| c["level"] != "exact")
    );
    assert!(
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["span"]["path"] == "Values.java" && c["target"].is_null())
    );
}

#[test]
fn java_inner_bindings_hide_imports_in_each_source_binding_form() {
    let repo = fixture_repo("java_hidden_names", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("Shadow.java"),
        r#"
package other;
import example /* path comment */ . util.Tools;
class Shadow {
    Object Tools;
    void field() { Tools.single(); }
    void parameter(Object Tools) { Tools.single(); }
    void local() { Object Tools = null; Tools.single(); }
    void lambda() { java.util.function.Consumer<Object> action = (Object Tools) -> Tools.single(); }
    void caught() { try { missing(); } catch (Exception Tools) { Tools.single(); } }
    void resource() { try (AutoCloseable Tools = missing()) { Tools.single(); } }
    void loop(Object[] items) { for (Object Tools : items) { Tools.single(); } }
    void pattern(Object item) { if (item instanceof Object Tools) { Tools.single(); } }
}
"#,
    )
    .unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls: Vec<_> = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["span"]["path"] == "Shadow.java"
                && c["id"].as_str().unwrap().contains("::Tools.single#")
        })
        .collect();
    assert_eq!(calls.len(), 8);
    assert!(calls.iter().all(|c| c["level"] != "exact"));
    assert!(
        json["processes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| !p["entry_point"].as_str().unwrap().contains("Shadow.java"))
    );
}

#[test]
fn java_method_names_are_separate_from_values_and_anonymous_receivers_are_opaque() {
    let repo = fixture_repo("java_method_namespace", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("App.java"),
        r#"
package example;
import external.Remote;
public class App {
    public void ping() {}
    public void run() { int ping = 1; ping(); }
    public void anonymous() {
        Remote remote = new Remote() {
            public void start() { ping(); this.ping(); }
        };
    }
}
"#,
    )
    .unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls = json["calls"].as_array().unwrap();
    let run = calls
        .iter()
        .find(|c| {
            c["id"]
                .as_str()
                .unwrap()
                .ends_with("example.App.run()::ping#0")
        })
        .unwrap();
    assert_eq!(run["level"], "exact");
    assert!(
        run["target"]
            .as_str()
            .unwrap()
            .ends_with("example.App.ping()")
    );
    let anonymous: Vec<_> = calls
        .iter()
        .filter(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("example.App.anonymous()")
                && c["id"].as_str().unwrap().contains("::(anonymous).ping#")
        })
        .collect();
    assert_eq!(anonymous.len(), 2);
    assert!(anonymous.iter().all(|c| c["level"] != "exact"));
}

#[test]
fn java_on_demand_imports_keep_ambiguity_and_explicit_imports_take_priority() {
    let repo = fixture_repo("java_import_priority", "conformance-java", Some(ORIGIN));
    for (path, source) in [
        (
            "one/Wild.java",
            "package one; public class Wild { public static void ping() {} }",
        ),
        (
            "two/Wild.java",
            "package two; public class Wild { public static void ping() {} }",
        ),
        (
            "other/Noise.java",
            "package other; public class Noise { public static void ping() {} }",
        ),
        (
            "client/Ambiguous.java",
            "package client; import one.*; import two.*; public class Ambiguous { public static void run() { Wild.ping(); } }",
        ),
        (
            "client/Explicit.java",
            "package client; import one.Wild; import two.*; import one.*; public class Explicit { public static void run() { Wild.ping(); } }",
        ),
        (
            "client/Choice.java",
            "package client; class Choice { public static void pick() {} }",
        ),
        (
            "java/lang/Choice.java",
            "package java.lang; public class Choice { public static void pick() {} }",
        ),
        (
            "java/lang/Implicit.java",
            "package java.lang; public class Implicit { public static void touch() {} }",
        ),
        (
            "client/Current.java",
            "package client; public class Current { public static void run() { Choice.pick(); } }",
        ),
        (
            "client/Language.java",
            "package client; public class Language { public static void run() { Implicit.touch(); } }",
        ),
        (
            "client/StaticAmbiguous.java",
            "package client; import static one.Wild.*; import static two.Wild.*; public class StaticAmbiguous { public static void run() { ping(); } }",
        ),
        (
            "client/StaticExplicit.java",
            "package client; import static one.Wild.ping; import static two.Wild.*; import static one.Wild.*; public class StaticExplicit { public static void run() { ping(); } }",
        ),
        (
            "client/NestedImported.java",
            "package client; import static example.Orders.*; public class NestedImported { public static void run() { new Nested(); } }",
        ),
        (
            "module-one/src/p/Tools.java",
            "package p; public class Tools { public static void go() {} }",
        ),
        (
            "module-two/src/p/Tools.java",
            "package p; public class Tools { public static void go() {} }",
        ),
        (
            "client/Duplicate.java",
            "package client; import p.Tools; public class Duplicate { public static void run() { Tools.go(); } }",
        ),
    ] {
        let path = repo.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, source).unwrap();
    }
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = |caller: &str| {
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["caller"].as_str().unwrap().ends_with(caller))
            .unwrap()
    };
    let duplicate = call("client.Duplicate.run()");
    assert_eq!(duplicate["level"], "ambiguous");
    let candidates = duplicate["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2);
    assert!(
        candidates[0]
            .as_str()
            .unwrap()
            .contains("module-one/src/p/Tools.java::p.Tools.go()")
    );
    assert!(
        candidates[1]
            .as_str()
            .unwrap()
            .contains("module-two/src/p/Tools.java::p.Tools.go()")
    );
    for caller in ["client.Ambiguous.run()", "client.StaticAmbiguous.run()"] {
        let found = call(caller);
        assert_eq!(found["level"], "ambiguous", "{caller}: {found}");
        let candidates: Vec<_> = found["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c.as_str().unwrap().rsplit("::").next().unwrap())
            .collect();
        assert_eq!(
            candidates,
            ["one.Wild.ping()", "two.Wild.ping()"],
            "{caller}"
        );
    }
    for (caller, target) in [
        ("client.Explicit.run()", "one.Wild.ping()"),
        ("client.StaticExplicit.run()", "one.Wild.ping()"),
        ("client.Current.run()", "client.Choice.pick()"),
        ("client.Language.run()", "java.lang.Implicit.touch()"),
        ("client.NestedImported.run()", "example.Orders.Nested"),
    ] {
        let found = call(caller);
        assert_eq!(found["level"], "exact", "{caller}: {found}");
        assert!(
            found["target"].as_str().unwrap().ends_with(target),
            "{caller}: {found}"
        );
    }
}

#[test]
fn java_type_use_annotation_values_stay_out_of_exported_signatures() {
    let repo = fixture_repo("java_type_use", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("Annotated.java"),
        r#"
package example;
import java.lang.annotation.Target;
import java.lang.annotation.ElementType;
@Target(ElementType.TYPE_USE)
@interface Tag { String value(); }
public class Annotated {
    public void generic(java.util.List<@Tag("generic-value") String> values) {}
    public void commented(java.util./* comment-value */List<String> values) {}
    public void array(String @Tag("array-value") [] values) {}
    public void after(String values @Tag("after-value") []) {}
    public void spread(String @Tag("spread-value") ... values) {}
    public void create() { new java.util.@Tag("new-value") ArrayList<String>(); }
}

record Tagged(java.util.List<@Tag("record-value") String> values) { Tagged {} }
"#,
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let export = engine.export().unwrap();
    for value in [
        "generic-value",
        "array-value",
        "after-value",
        "spread-value",
        "new-value",
        "record-value",
        "comment-value",
    ] {
        assert!(
            !export.contains(value),
            "annotation value escaped into an indexed name: {value}"
        );
    }
    let json: Value = serde_json::from_str(&export).unwrap();
    for name in [
        "example.Annotated.generic(java.util.List<String>)",
        "example.Annotated.commented(java.util.List<String>)",
        "example.Annotated.array(String[])",
        "example.Annotated.after(String[])",
        "example.Annotated.spread(String...)",
        "example.Tagged.Tagged(java.util.List<String>)",
    ] {
        assert!(
            json["symbols"]
                .as_array()
                .unwrap()
                .iter()
                .any(|s| s["id"].as_str().unwrap().ends_with(name)),
            "{name}"
        );
    }
    assert_eq!(export, engine.export().unwrap());
}

#[test]
fn java_comments_do_not_change_overload_argument_counts() {
    let repo = fixture_repo("java_argument_comments", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("Comments.java"),
        r#"
package example;
class Comments {
    static void pick() {}
    static void pick(String value) {}
    static void pair(String first, String second) {}
    static void pair(String first, String second, String third) {}
    static void run() { pick(/* none */); pair("first", /* middle */ "second"); }
}
"#,
    )
    .unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls: Vec<_> = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("example.Comments.run()")
        })
        .collect();
    assert_eq!(calls.len(), 2);
    for (suffix, target) in [
        ("::pick#0", "example.Comments.pick()"),
        ("::pair#0", "example.Comments.pair(String,String)"),
    ] {
        let call = calls
            .iter()
            .find(|c| c["id"].as_str().unwrap().ends_with(suffix))
            .unwrap();
        assert_eq!(call["level"], "exact");
        assert!(call["target"].as_str().unwrap().ends_with(target), "{call}");
    }
}

#[test]
fn java_enum_and_interface_values_hide_imported_types() {
    let repo = fixture_repo("java_constant_bindings", "conformance-java", Some(ORIGIN));
    for (path, source) in [
        (
            "EnumValue.java",
            "package other; import example.util.Tools; enum EnumValue { Tools; void single() {} void run() { Tools.single(); } }",
        ),
        (
            "InterfaceValue.java",
            "package other; import example.util.Tools; interface InterfaceValue { InterfaceValue Tools = null; default void single() {} default void run() { Tools.single(); } }",
        ),
    ] {
        std::fs::write(repo.join(path), source).unwrap();
    }
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = |caller: &str| {
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["caller"].as_str().unwrap().ends_with(caller))
            .unwrap()
            .clone()
    };
    let enumerated = call("other.EnumValue.run()");
    assert_ne!(enumerated["level"], "exact", "{enumerated}");
    // The constant's written type, not the import, owns the method.
    let typed = call("other.InterfaceValue.run()");
    assert_eq!(typed["level"], "exact", "{typed}");
    assert!(
        typed["target"]
            .as_str()
            .unwrap()
            .ends_with("other.InterfaceValue.single()"),
        "{typed}"
    );
}

#[test]
fn java_fully_qualified_static_calls_respect_value_shadowing() {
    let repo = fixture_repo("java_qualified_static", "conformance-java", Some(ORIGIN));
    std::fs::write(repo.join("Qualified.java"), "package other; class Qualified { void run() { example.util.Tools.single(); } void hidden(Object example) { example.util.Tools.single(); } }").unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = |caller: &str| {
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["caller"].as_str().unwrap().ends_with(caller))
            .unwrap()
    };
    assert_eq!(call("other.Qualified.run()")["level"], "exact");
    assert!(
        call("other.Qualified.run()")["target"]
            .as_str()
            .unwrap()
            .ends_with("example.util.Tools.single()")
    );
    assert_ne!(call("other.Qualified.hidden(Object)")["level"], "exact");
}

#[test]
fn java_same_named_factories_and_nested_constructors_stay_separate() {
    let repo = fixture_repo("java_factory_namespace", "conformance-java", Some(ORIGIN));
    std::fs::write(repo.join("Factory.java"), "package example; class Factory { static class Task { Task() {} } static Task Task() { return new Task(); } static void run() { Task(); new Task(); } }").unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls: Vec<_> = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["span"]["path"] == "Factory.java")
        .collect();
    assert_eq!(calls.len(), 3);
    for call in calls {
        let suffix = if call["id"].as_str().unwrap().ends_with("::Task#0")
            && call["caller"].as_str().unwrap().ends_with("Factory.run()")
        {
            "example.Factory.Task()"
        } else {
            "example.Factory.Task.Task()"
        };
        assert_eq!(call["level"], "exact", "{call}");
        assert!(call["target"].as_str().unwrap().ends_with(suffix), "{call}");
    }
}

#[test]
fn java_constructor_names_ignore_methods_and_value_bindings() {
    let repo = fixture_repo("java_constructor_names", "conformance-java", Some(ORIGIN));
    for (path, source) in [
        (
            "Task.java",
            "package other; public class Task { public Task() {} }",
        ),
        (
            "Creator.java",
            "package example; import other.Task; class Creator { static void Task() {} void run(Object Task) { new Task(); } }",
        ),
    ] {
        std::fs::write(repo.join(path), source).unwrap();
    }
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("example.Creator.run(Object)")
        })
        .unwrap();
    assert_eq!(call["level"], "exact", "{call}");
    assert!(
        call["target"]
            .as_str()
            .unwrap()
            .ends_with("other.Task.Task()"),
        "{call}"
    );
}

#[test]
fn java_package_paths_cannot_bypass_a_shadowing_value() {
    let repo = fixture_repo("java_package_value", "conformance-java", Some(ORIGIN));
    for (path, source) in [
        (
            "Tools.java",
            "package provider; public class Tools { public static void single() {} }",
        ),
        (
            "Holder.java",
            "package provider; class Holder { Holder Tools; void single() {} }",
        ),
        (
            "Caller.java",
            "package provider; class Caller { void run(Holder provider) { provider.Tools.single(); } }",
        ),
    ] {
        std::fs::write(repo.join(path), source).unwrap();
    }
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("provider.Caller.run(Holder)")
        })
        .unwrap();
    // The field's written type, not the package path, owns the method.
    assert_eq!(call["level"], "exact", "{call}");
    assert!(
        call["target"]
            .as_str()
            .unwrap()
            .ends_with("provider.Holder.single()"),
        "{call}"
    );
}

#[test]
fn java_omitted_nested_types_cannot_construct_an_outer_type() {
    let repo = fixture_repo("java_omitted_constructor", "conformance-java", Some(ORIGIN));
    let mut source = String::from("package hostile; class Root { static class Task { Task() {} }");
    for _ in 0..128 {
        source.push_str("class N {");
    }
    source.push_str("class Task { Task() {} } void run() { new Task(); }");
    source.push_str(&"}".repeat(129));
    std::fs::write(repo.join("Root.java"), source).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["span"]["path"] == "Root.java")
        .unwrap();
    assert_ne!(call["level"], "exact", "{call}");
}

#[test]
fn java_constructors_and_class_named_methods_have_distinct_identities() {
    let repo = fixture_repo(
        "java_constructor_identity",
        "conformance-java",
        Some(ORIGIN),
    );
    std::fs::write(repo.join("Task.java"), "package example; class Task { Task() {} void Task() {} void run() { new Task(); Task(); } }").unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let calls: Vec<_> = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("example.Task.run()")
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|c| c["level"] == "exact"));
    assert_ne!(calls[0]["target"], calls[1]["target"]);
    for call in calls {
        assert_eq!(
            json["symbols"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|s| s["id"] == call["target"])
                .count(),
            1
        );
    }
}

#[test]
fn java_malformed_type_literals_stay_out_of_exported_names() {
    let repo = fixture_repo("java_type_literals", "conformance-java", Some(ORIGIN));
    std::fs::write(
        repo.join("Malformed.java"),
        r#"
package example;
class Malformed {
    void string(java.util.List<"literal-type-value"> value) {}
    void character(java.util.List<'§'> value) {}
    void number(java.util.List<917263540> value) {}
}
"#,
    )
    .unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    for literal in ["literal-type-value", "§", "917263540"] {
        assert!(
            !export.contains(literal),
            "type error persisted a literal: {literal}"
        );
    }
    assert!(export.contains("example.Orders.start()"));
}

#[test]
fn java_external_constructor_cannot_bind_static_method_overloads() {
    let repo = fixture_repo(
        "java_external_constructor",
        "conformance-java",
        Some(ORIGIN),
    );
    std::fs::write(repo.join("Helpers.java"), "package helpers; public class Helpers { public static void Task() {} public static void Task(int value) {} }").unwrap();
    std::fs::write(repo.join("Caller.java"), "package client; import external.*; import static helpers.Helpers.*; class Caller { void run() { new Task(); } }").unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("client.Caller.run()")
        })
        .unwrap();
    assert_ne!(call["level"], "exact", "{call}");
    assert!(call["target"].is_null(), "{call}");
}

#[test]
fn java_qualification_expansion_discards_the_whole_file_and_keeps_other_files() {
    let repo = fixture_repo(
        "java_qualification_budget",
        "conformance-java",
        Some(ORIGIN),
    );
    let package = format!("p{}", "a".repeat(4096));
    let mut source = format!(
        "package {package}; public class Expanded {{ public static void overloaded(String value) {{}} "
    );
    for n in 0..600 {
        source.push_str(&format!("public static void method{n}() {{}} "));
    }
    source.push_str("public static void overloaded(int value) {} }");
    std::fs::write(repo.join("Expanded.java"), source).unwrap();
    std::fs::write(repo.join("BudgetCaller.java"), format!("package example; import {package}.Expanded; public class BudgetCaller {{ public static void run() {{ Expanded.overloaded(\"value\"); }} }}")).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let symbols = json["symbols"].as_array().unwrap();
    assert!(symbols.iter().all(|s| s["span"]["path"] != "Expanded.java"));
    assert!(symbols.iter().any(|s| {
        s["id"]
            .as_str()
            .unwrap()
            .ends_with("example.Orders.start()")
    }));
    assert!(symbols.iter().any(|s| {
        s["id"]
            .as_str()
            .unwrap()
            .ends_with("example.BudgetCaller.run()")
    }));
    let called = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| {
            c["caller"]
                .as_str()
                .unwrap()
                .ends_with("example.BudgetCaller.run()")
        })
        .unwrap();
    assert!(called["target"].is_null());
    assert!(called["level"].is_null());
}

#[test]
fn kotlin() {
    conform(
        "conformance-kotlin",
        &[
            "scripts/seed.kts::seed()",
            "src/main/kotlin/example/Members.kt::example.Base",
            "src/main/kotlin/example/Members.kt::example.Base.log(String)",
            "src/main/kotlin/example/Members.kt::example.log(String)",
            "src/main/kotlin/example/Members.kt::example.origin()",
            "src/main/kotlin/example/Members.kt::example.Derived",
            "src/main/kotlin/example/Members.kt::example.Derived.go()",
            "src/main/kotlin/example/Members.kt::example.Derived.first()",
            "src/main/kotlin/example/Members.kt::example.Derived.Companion",
            "src/main/kotlin/example/Members.kt::example.Derived.Companion.origin()",
            "src/main/kotlin/example/Members.kt::example.Logged",
            "src/main/kotlin/example/Members.kt::example.Logged.go()",
            "src/main/kotlin/example/Members.kt::example.Logged.self()",
            "src/main/kotlin/example/Members.kt::example.Logged.own()",
            "src/main/kotlin/example/Members.kt::example.Quiet",
            "src/main/kotlin/example/Members.kt::example.Quiet.go()",
            "src/main/kotlin/example/Members.kt::example.Quiet.own()",
            "src/main/kotlin/example/Members.kt::example.Quiet.make()",
            "src/main/kotlin/example/Members.kt::example.Quiet.Companion",
            "src/main/kotlin/example/Members.kt::example.Quiet.Companion.build()",
            "src/main/kotlin/example/Members.kt::example.util()",
            "src/main/kotlin/example/Members.kt::example.Init",
            "src/main/kotlin/example/Members.kt::example.Level",
            "src/main/kotlin/example/Members.kt::example.Level.label()",
            "src/main/kotlin/example/Main.kt::example.main(Array<String>)",
            "src/main/kotlin/example/Main.kt::example.Runner",
            "src/main/kotlin/example/Main.kt::example.Runner.main()",
            "src/main/kotlin/example/Main.kt::example.Launcher",
            "src/main/kotlin/example/Main.kt::example.Launcher.main()",
            "src/main/kotlin/example/Main.kt::example.String.main()",
            "src/main/kotlin/example/Shapes.kt::example.Point.fromOrigin()",
            "src/main/kotlin/example/util/Hidden.kt::example.util.Hidden.visible()",
            "src/main/kotlin/example/util/Hidden.kt::example.util.viaPackage()",
            "src/main/kotlin/example/calls/Other.kt::example.calls.greet()",
            "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy",
            "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()",
            "src/main/kotlin/example/Same.kt::example.notSecret()",
            "src/main/kotlin/example/Same.kt::example.same()",
            "src/main/kotlin/example/Shapes.kt::example.Box",
            "src/main/kotlin/example/Shapes.kt::example.Box.Factory",
            "src/main/kotlin/example/Shapes.kt::example.Box.Factory.create()",
            "src/main/kotlin/example/Shapes.kt::example.Box.label()",
            "src/main/kotlin/example/Shapes.kt::example.Box.later()",
            "src/main/kotlin/example/Shapes.kt::example.Box.name()",
            "src/main/kotlin/example/Shapes.kt::example.Box.relabel()",
            "src/main/kotlin/example/Shapes.kt::example.Color",
            "src/main/kotlin/example/Shapes.kt::example.Marker",
            "src/main/kotlin/example/Shapes.kt::example.Named",
            "src/main/kotlin/example/Shapes.kt::example.Named.name()",
            "src/main/kotlin/example/Shapes.kt::example.Point",
            "src/main/kotlin/example/Shapes.kt::example.Point.Companion",
            "src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin()",
            "src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin(Int)",
            "src/main/kotlin/example/Shapes.kt::example.Point.moved(Int)",
            "src/main/kotlin/example/Shapes.kt::example.Point.norm()",
            "src/main/kotlin/example/Shapes.kt::example.Point.scaled(Int)",
            "src/main/kotlin/example/Shapes.kt::example.Point.twice()",
            "src/main/kotlin/example/Shapes.kt::example.Registry",
            "src/main/kotlin/example/Shapes.kt::example.Registry.all()",
            "src/main/kotlin/example/Shapes.kt::example.Registry.find(String)",
            "src/main/kotlin/example/Shapes.kt::example.Registry.find(String,Point)",
            "src/main/kotlin/example/Shapes.kt::example.Shape",
            "src/main/kotlin/example/Shapes.kt::example.Shape.Square",
            "src/main/kotlin/example/Shapes.kt::example.Shape.Square.Corner",
            "src/main/kotlin/example/Shapes.kt::example.Shape.Square.Corner.area()",
            "src/main/kotlin/example/Shapes.kt::example.Shape.Square.area()",
            "src/main/kotlin/example/Shapes.kt::example.Shape.area()",
            "src/main/kotlin/example/Shapes.kt::example.String.shout()",
            "src/main/kotlin/example/Shapes.kt::example.calls()",
            "src/main/kotlin/example/Shapes.kt::example.defaults(Int,Int)",
            "src/main/kotlin/example/Shapes.kt::example.greeting",
            "src/main/kotlin/example/Shapes.kt::example.helper()",
            "src/main/kotlin/example/Shapes.kt::example.helper(Int)",
            "src/main/kotlin/example/Shapes.kt::example.helper(String)",
            "src/main/kotlin/example/Shapes.kt::example.hiding((Int)->Int,List<()->Int>)",
            "src/main/kotlin/example/Shapes.kt::example.hiding((Int)->Int,List<()->Int>).inside()",
            "src/main/kotlin/example/Shapes.kt::example.inside()",
            "src/main/kotlin/example/Shapes.kt::example.many(vararg Int)",
            "src/main/kotlin/example/Shapes.kt::example.open()",
            "src/main/kotlin/example/Shapes.kt::example.secret()",
            "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()",
            "src/main/kotlin/example/calls/Imports.kt::example.calls.qualified()",
            "src/main/kotlin/example/calls/Imports.kt::example.calls.shadowed(()->Int)",
            "src/main/kotlin/example/calls/Imports.kt::example.calls.unknown()",
            "src/main/kotlin/example/util/Hidden.kt::example.util.FilePrivate",
            "src/main/kotlin/example/util/Hidden.kt::example.util.Hidden",
            "src/main/kotlin/example/util/Hidden.kt::example.util.Hidden.closed()",
            "src/main/kotlin/example/util/Hidden.kt::example.util.Hidden.shared()",
            "src/main/kotlin/example/util/Tools.kt::example.util.Tools",
            "src/main/kotlin/example/util/Tools.kt::example.util.Tools.doubled()",
            "src/main/kotlin/example/util/Tools.kt::example.util.Tools.single()",
            "src/main/kotlin/example/util/Tools.kt::example.util.internalTool()",
            "src/main/kotlin/example/util/Tools.kt::example.util.tool()",
            "src/main/kotlin/example/util/Tools.kt::example.util.tool(Int,Int)",
            "src/main/kotlin/example/Receivers.kt::example.Store",
            "src/main/kotlin/example/Receivers.kt::example.Store.get(String)",
            "src/main/kotlin/example/Receivers.kt::example.Store.put(String,String)",
            "src/main/kotlin/example/Receivers.kt::example.Cache",
            "src/main/kotlin/example/Receivers.kt::example.Cache.get(String)",
            "src/main/kotlin/example/Receivers.kt::example.Cache.put(String,String)",
            "src/main/kotlin/example/Receivers.kt::example.Cache.size()",
            "src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)",
            "src/main/kotlin/example/Receivers.kt::example.Cache.clear()",
            "src/main/kotlin/example/Receivers.kt::example.Cache.wipe()",
            "src/main/kotlin/example/Receivers.kt::example.Cache.add(String)",
            "src/main/kotlin/example/Receivers.kt::example.Cache.add(Int)",
            "src/main/kotlin/example/Receivers.kt::example.Disk",
            "src/main/kotlin/example/Receivers.kt::example.Disk.flush()",
            "src/main/kotlin/example/Receivers.kt::example.Disk.inherited()",
            "src/main/kotlin/example/Receivers.kt::example.build()",
            "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)",
            "src/main/kotlin/example/Receivers.kt::example.local()",
            "src/main/kotlin/example/Receivers.kt::example.String.loud()",
            "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)",
            "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)",
            "src/main/kotlin/example/Receivers.kt::example.Mixed",
            "src/main/kotlin/example/Receivers.kt::example.Mixed.get(String)",
            "src/main/kotlin/example/Receivers.kt::example.Mixed.put(String,String)",
            "src/main/kotlin/example/Receivers.kt::example.outsiders(Outside,Mixed,List<Int>)",
            "src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.describe()",
            "src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.describe(Boolean)",
            "src/main/kotlin/example/ext/Extensions.kt::example.ext.Store.tag()",
            "src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.tag()",
            "src/main/kotlin/example/Receivers.kt::example.scoped(String)",
            "src/main/kotlin/example/Receivers.kt::example.Outside.spawn()",
            "src/main/kotlin/example/Receivers.kt::example.Outside.items()",
            "src/main/kotlin/example/Receivers.kt::example.Outside.orders()",
            "src/main/kotlin/example/Receivers.kt::example.Map.Entry.entryLabel()",
            "src/main/kotlin/example/Receivers.kt::example.opaque(Outside,Map<String,Int>)",
            "src/main/kotlin/example/Receivers.kt::example.Any.toJson()",
            "src/main/kotlin/example/Receivers.kt::example.T.logged()",
            "src/main/kotlin/example/Receivers.kt::example.CharSequence.blankish()",
            "src/main/kotlin/example/Receivers.kt::example.general(Cache,String)",
            "src/main/kotlin/example/Receivers.kt::example.Outside.pretty()",
            "src/main/kotlin/example/Receivers.kt::example.String.pretty()",
            "src/main/kotlin/example/Receivers.kt::example.called()",
            "src/main/kotlin/example/Receivers.kt::example.nestedIt(Cache,List<Disk>)",
            "src/main/kotlin/example/Receivers.kt::example.localFirst(Cache)",
            "src/main/kotlin/example/Receivers.kt::example.localFirst(Cache).size()",
            "src/main/kotlin/example/Receivers.kt::example.unknownReceiver()",
            "src/main/kotlin/example/other/Others.kt::example.other.Cache.describe()",
            "src/main/kotlin/example/other/Others.kt::example.other.String.reversed()",
            "src/main/java/example/legacy/Port.java::example.legacy.Port",
            "src/main/java/example/legacy/Port.java::example.legacy.Port.ext(int)",
            "src/main/java/example/legacy/Port.java::example.legacy.Port.use(Port,String)",
            "src/main/kotlin/example/legacy/PortExt.kt::example.legacy.Port.ext(String)",
            "src/main/kotlin/example/legacy/PortExt.kt::example.legacy.String.length()",
            "src/main/kotlin/example/web/Api.kt::example.web.Api",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.Companion",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.list()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.fetch(String)",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.create()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.replace()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.flag()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.remove()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.several()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.single()",
            "src/main/kotlin/example/web/OrderController.kt::example.web.OrderController.any()",
            "src/main/kotlin/example/web/PageController.kt::example.web.PageController",
            "src/main/kotlin/example/web/PageController.kt::example.web.PageController.page()",
            "src/main/kotlin/example/web/StarController.kt::example.web.StarController",
            "src/main/kotlin/example/web/StarController.kt::example.web.StarController.one()",
            "src/main/kotlin/example/web/StarController.kt::example.web.StarController.removeAll()",
            "src/main/kotlin/example/web/VarargController.kt::example.web.VarargController",
            "src/main/kotlin/example/web/VarargController.kt::example.web.VarargController.pair()",
            "src/main/kotlin/example/web/VarargController.kt::example.web.VarargController.listed()",
            "src/main/kotlin/example/web/VarargController.kt::example.web.VarargController.put()",
        ],
        &["src/main/kotlin/example/Main.kt::example.main(Array<String>)"],
        &[
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::helper#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.helper()"],
            ),
            (
                "src/main/kotlin/example/Same.kt::example.same()::helper#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.helper()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::helper#1",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Shapes.kt::example.helper(Int)",
                    "src/main/kotlin/example/Shapes.kt::example.helper(String)",
                ],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Registry.find(String,Point)::find#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.find(String)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::defaults#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.defaults(Int,Int)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::defaults#1",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.defaults(Int,Int)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::many#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.many(vararg Int)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Box.relabel()::this.label#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box.label()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Box.name()::label#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box.label()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Registry.find#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.find(String)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Registry.find#1",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.find(String,Point)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Point.origin#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Point.origin#1",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin(Int)"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Point.twice()::Point.origin#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Box.create#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box.Factory.create()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Box.Factory.create#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box.Factory.create()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Point#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::Shape.Square#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Shape.Square"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example::Marker#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Marker"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::(anonymous).helper#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Shapes.kt::example.helper(Int)",
                    "src/main/kotlin/example/Shapes.kt::example.helper(String)",
                ],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Box.later()::(anonymous).label#0",
                INFERRED,
                &["src/main/kotlin/example/Shapes.kt::example.Box.label()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.greeting::Registry.all#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.all()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.open()::secret#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.secret()"],
            ),
            (
                "src/main/kotlin/example/Same.kt::example.notSecret()::secret#0",
                INFERRED,
                &["src/main/kotlin/example/Shapes.kt::example.secret()"],
            ),
            (
                "src/main/kotlin/example/Same.kt::example.same()::inside#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.inside()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::internalTool#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.internalTool()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::Point.origin#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::Lookup.find#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.find(String)"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::tool#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.tool()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::run#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.tool(Int,Int)"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::Tools.single#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.Tools.single()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.qualified()::example.util.Tools.doubled#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.Tools.doubled()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::Legacy.make#0",
                EXACT,
                &["src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()"],
            ),
            (
                "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()::Box#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box"],
            ),
            (
                "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()::box.label#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Box.label()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.hiding((Int)->Int,List<()->Int>)::inside#0",
                EXACT,
                &[
                    "src/main/kotlin/example/Shapes.kt::example.hiding((Int)->Int,List<()->Int>).inside()",
                ],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.hiding((Int)->Int,List<()->Int>)::same#0",
                INFERRED,
                &["src/main/kotlin/example/Same.kt::example.same()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.shadowed(()->Int)::run#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.unknown()::outside#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.unknown()::Missing.call#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.calls()::println#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Derived.go()::log#0",
                EXACT,
                &["src/main/kotlin/example/Members.kt::example.Base.log(String)"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Derived.first()::origin#0",
                EXACT,
                &["src/main/kotlin/example/Members.kt::example.Derived.Companion.origin()"],
            ),
            (
                "src/main/kotlin/example/Shapes.kt::example.Point.fromOrigin()::origin#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Point.Companion.origin()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Logged.self()::this.own#0",
                INFERRED,
                &["src/main/kotlin/example/Members.kt::example.Logged.own()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Quiet.go()::this.own#0",
                EXACT,
                &["src/main/kotlin/example/Members.kt::example.Quiet.own()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Quiet.make()::Quiet.build#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Logged.go()::log#0",
                INFERRED,
                &["src/main/kotlin/example/Members.kt::example.log(String)"],
            ),
            (
                "src/main/kotlin/example/util/Hidden.kt::example.util.viaPackage()::tool#0",
                EXACT,
                &["src/main/kotlin/example/util/Tools.kt::example.util.tool()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Init::helper#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.helper()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Init::local#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Level::helper#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.helper()"],
            ),
            (
                "src/main/kotlin/example/Members.kt::example.Level::(anonymous).label#0",
                INFERRED,
                &["src/main/kotlin/example/Members.kt::example.Level.label()"],
            ),
            (
                "src/main/kotlin/example/calls/Imports.kt::example.calls.imported()::greet#0",
                INFERRED,
                &["src/main/kotlin/example/calls/Other.kt::example.calls.greet()"],
            ),
            (
                "scripts/run.kts::seed#0",
                INFERRED,
                &["scripts/seed.kts::seed()"],
            ),
            (
                "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()::Registry.INSTANCE.all#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.all()"],
            ),
            (
                "src/main/java/example/legacy/Legacy.java::example.legacy.Legacy.make()::Registry.INSTANCE.find#0",
                EXACT,
                &["src/main/kotlin/example/Shapes.kt::example.Registry.find(String)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::cache.clear#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.clear()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::store.put#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Store.put(String,String)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::spare.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::spare.size#1",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::boxed.wipe#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::this.cache.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.flush()::made.wipe#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk::plain.wipe#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Disk.inherited()::this.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::cache.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::cache.size#1",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::cache.add#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Receivers.kt::example.Cache.add(Int)",
                    "src/main/kotlin/example/Receivers.kt::example.Cache.add(String)",
                ],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::store.get#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Store.get(String)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::disk.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::disk.get#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.get(String)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.parameter(Cache,Store,Disk)::disk.cache.wipe#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.local()::declared.clear#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.clear()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.local()::created.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.local()::created.size#1",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.local()::guessed.clear#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.clear()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.local()::guessed.get#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Receivers.kt::example.Cache.get(String)",
                    "src/main/kotlin/example/Receivers.kt::example.Mixed.get(String)",
                    "src/main/kotlin/example/Receivers.kt::example.Store.get(String)",
                ],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::cache.describe#0",
                EXACT,
                &["src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.describe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::cache.describe#1",
                EXACT,
                &["src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.describe(Boolean)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::store.tag#0",
                EXACT,
                &["src/main/kotlin/example/ext/Extensions.kt::example.ext.Store.tag()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::disk.tag#0",
                EXACT,
                &["src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.tag()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::disk.describe#0",
                EXACT,
                &["src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.describe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::text.loud#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.String.loud()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::text.shout#0",
                INFERRED,
                &["src/main/kotlin/example/Shapes.kt::example.String.shout()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.extensions(Cache,Store,Disk,String)::(call_expression).tag#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/ext/Extensions.kt::example.ext.Cache.tag()",
                    "src/main/kotlin/example/ext/Extensions.kt::example.ext.Store.tag()",
                ],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::this.clear#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.clear()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::build#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.build()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::wipe#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::flush#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Disk.flush()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::inherited#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Disk.inherited()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::it.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::value.flush#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Disk.flush()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::build#1",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.build()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scopes(Cache,Disk,Store)::it.wipe#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.outsiders(Outside,Mixed,List<Int>)::foreign.clear#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.outsiders(Outside,Mixed,List<Int>)::list.wipe#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.outsiders(Outside,Mixed,List<Int>)::mixed.put#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Receivers.kt::example.Mixed.put(String,String)",
                    "src/main/kotlin/example/Receivers.kt::example.Store.put(String,String)",
                ],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.outsiders(Outside,Mixed,List<Int>)::mixed.size#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.scoped(String)::text.reversed#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Outside.orders()::items#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Outside.items()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.Outside.orders()::this.items#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Outside.items()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.opaque(Outside,Map<String,Int>)::foreign.items#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Outside.items()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.opaque(Outside,Map<String,Int>)::Outside.spawn#0",
                None,
                &[],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.opaque(Outside,Map<String,Int>)::entries.entryLabel#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Map.Entry.entryLabel()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.general(Cache,String)::cache.toJson#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Any.toJson()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.general(Cache,String)::text.toJson#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Any.toJson()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.general(Cache,String)::text.logged#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.T.logged()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.general(Cache,String)::text.blankish#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.CharSequence.blankish()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.called()::shown.pretty#0",
                AMBIGUOUS,
                &[
                    "src/main/kotlin/example/Receivers.kt::example.Outside.pretty()",
                    "src/main/kotlin/example/Receivers.kt::example.String.pretty()",
                ],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.called()::held.wipe#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.wipe()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.nestedIt(Cache,List<Disk>)::it.size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.Cache.size(Int)"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.nestedIt(Cache,List<Disk>)::it.flush#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.Disk.flush()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.localFirst(Cache)::size#0",
                EXACT,
                &["src/main/kotlin/example/Receivers.kt::example.localFirst(Cache).size()"],
            ),
            (
                "src/main/kotlin/example/Receivers.kt::example.unknownReceiver()::(call_expression).loud#0",
                INFERRED,
                &["src/main/kotlin/example/Receivers.kt::example.String.loud()"],
            ),
            (
                "src/main/java/example/legacy/Port.java::example.legacy.Port.use(Port,String)::port.ext#0",
                EXACT,
                &["src/main/java/example/legacy/Port.java::example.legacy.Port.ext(int)"],
            ),
            (
                "src/main/java/example/legacy/Port.java::example.legacy.Port.use(Port,String)::s.length#0",
                None,
                &[],
            ),
            (
                "scripts/seed.kts::seed#0",
                EXACT,
                &["scripts/seed.kts::seed()"],
            ),
        ],
    );
}

/// A `private` or `internal` member is no export door, and a public one is (export doors are
/// the processes of kind `export`).
#[test]
fn kotlin_private_and_internal_members_are_no_export_doors() {
    let repo = fixture_repo("kotlin_doors", "conformance-kotlin", Some(ORIGIN));
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let doors: Vec<&str> = (json["processes"].as_array().unwrap().iter())
        .filter(|p| p["kind"] == "export")
        .map(|p| p["entry_point"].as_str().unwrap())
        .collect();
    let door = |name: &str| doors.iter().any(|d| d.ends_with(name));
    assert!(door("::example.util.Hidden.visible()"), "{doors:?}");
    assert!(!door("::example.util.Hidden.shared()"), "{doors:?}");
    assert!(!door("::example.util.Hidden.closed()"), "{doors:?}");
}

/// Search finds an extension function by name.
#[test]
fn kotlin_search_finds_an_extension_function() {
    let repo = fixture_repo(
        "kotlin_search_extension",
        "conformance-kotlin",
        Some(ORIGIN),
    );
    assert_golden(
        "conformance-kotlin.search-norm.txt",
        &Engine::open(&repo)
            .unwrap()
            .search("norm", None, None)
            .unwrap(),
    );
}

/// Context lists a function's callers and callees.
#[test]
fn kotlin_context_lists_callers_and_callees() {
    let repo = fixture_repo("kotlin_context_helper", "conformance-kotlin", Some(ORIGIN));
    assert_golden(
        "conformance-kotlin.context-helper.txt",
        &Engine::open(&repo)
            .unwrap()
            .context("example.helper()", None)
            .unwrap(),
    );
}

/// Context lists a method's callers with their levels and reasons: typed receivers and scope
/// functions exact, an untyped receiver a name match.
#[test]
fn kotlin_context_lists_callers_by_receiver_type() {
    let repo = fixture_repo("kotlin_context_wipe", "conformance-kotlin", Some(ORIGIN));
    assert_golden(
        "conformance-kotlin.context-wipe.txt",
        &Engine::open(&repo)
            .unwrap()
            .context("example.Cache.wipe()", None)
            .unwrap(),
    );
}

/// Tracked lambdas nested past the definition depth, and long `apply` chains, stay bounded.
#[test]
fn kotlin_hostile_scope_function_nesting_remains_bounded() {
    let repo = fixture_repo("kotlin_scopes_depth", "conformance-kotlin", Some(ORIGIN));
    let mut nested = String::from("package hostile\nclass Box { fun size() = 0 }\n");
    nested.push_str("fun deep(box: Box) {\n");
    for _ in 0..300 {
        nested.push_str("box.apply {\n");
    }
    nested.push_str("size()\n");
    nested.push_str(&"}\n".repeat(300));
    nested.push_str("}\n");
    let chain = format!(
        "fun chain() {{ Box(){}.size() }}\n",
        ".apply { }".repeat(300)
    );
    std::fs::write(repo.join("Nested.kt"), nested + &chain).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let size: Vec<&Value> = (json["calls"].as_array().unwrap().iter())
        .filter(|c| c["span"]["path"] == "Nested.kt")
        .filter(|c| c["id"].as_str().unwrap().contains("::(anonymous).size#"))
        .collect();
    // The innermost call is past the depth where lambdas are scopes: a name match.
    assert!(size.iter().any(|c| c["level"] == "inferred"), "{size:?}");
}

/// Thousands of `apply` lambdas on one class index in time linear in their number: a lambda's
/// scope adds no members to its receiver's class, so no lookup of the class reads them all.
#[test]
fn kotlin_scope_functions_on_one_class_stay_linear_in_width() {
    const WIDTH: usize = 4000;
    let repo = fixture_repo("kotlin_scopes_width", "conformance-kotlin", Some(ORIGIN));
    let mut wide = String::from("package wide\nclass Foo { fun size() = 0 }\n");
    for i in 0..WIDTH {
        wide.push_str(&format!(
            "fun f{i}(foo: Foo) {{ foo.apply {{ size() }} }}\n"
        ));
    }
    std::fs::write(repo.join("Wide.kt"), wide).unwrap();
    let started = std::time::Instant::now();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let took = started.elapsed();
    let json: Value = serde_json::from_str(&export).unwrap();
    let exact = (json["calls"].as_array().unwrap().iter())
        .filter(|c| c["span"]["path"] == "Wide.kt" && c["level"] == "exact")
        .filter(|c| c["target"] == format!("{REPO}::Wide.kt::wide.Foo.size()"))
        .count();
    assert_eq!(exact, WIDTH);
    // Quadratic work took 70 s here in debug; linear work, a few.
    eprintln!("kotlin scope functions, {WIDTH} wide: {took:?}");
    assert!(took < std::time::Duration::from_secs(30), "{took:?}");
}

#[test]
fn kotlin_hostile_definition_value_and_member_depths_remain_bounded() {
    let repo = fixture_repo("kotlin_depths", "conformance-kotlin", Some(ORIGIN));
    let mut deep = String::from("package hostile\nclass Deep { fun ping() {}\n");
    for n in 0..132 {
        deep.push_str(&format!("class Inner{n} {{\n"));
    }
    deep.push_str("fun ping() {}\nfun leaf() { ping(); this.ping(); missing() }\n");
    deep.push_str(&"}\n".repeat(133));
    std::fs::write(repo.join("Deep.kt"), deep).unwrap();
    let expression = format!("{}\"value\"{}", "(".repeat(1024), ")".repeat(1024));
    let member = format!("root{}.send()", ".part".repeat(300));
    let values = format!("package hostile\nval V = {expression}\nfun run() {{ {member} }}\n");
    std::fs::write(repo.join("Values.kt"), values).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let definitions = json["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|s| s["span"]["path"] == "Deep.kt")
        .count();
    assert_eq!(definitions, 129);
    assert!(
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["span"]["path"] == "Deep.kt")
            .all(|c| c["level"] != "exact")
    );
    assert!(
        json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["span"]["path"] == "Values.kt" && c["target"].is_null())
    );
}

#[test]
fn kotlin_qualification_expansion_discards_the_whole_file_and_keeps_other_files() {
    let repo = fixture_repo(
        "kotlin_qualification_budget",
        "conformance-kotlin",
        Some(ORIGIN),
    );
    let package = format!("p{}", "a".repeat(4096));
    let mut source =
        format!("package {package}\nobject Expanded {{\nfun overloaded(value: String) {{}}\n");
    for n in 0..600 {
        source.push_str(&format!("fun method{n}() {{}}\n"));
    }
    source.push_str("fun overloaded(value: Int) {}\n}\n");
    std::fs::write(repo.join("Expanded.kt"), source).unwrap();
    let caller = format!(
        "package example\nimport {package}.Expanded\nfun budget() {{ Expanded.overloaded(\"value\") }}\n"
    );
    std::fs::write(repo.join("Budget.kt"), caller).unwrap();
    let json: Value =
        serde_json::from_str(&Engine::open(&repo).unwrap().export().unwrap()).unwrap();
    let symbols = json["symbols"].as_array().unwrap();
    assert!(symbols.iter().all(|s| s["span"]["path"] != "Expanded.kt"));
    assert!(
        symbols
            .iter()
            .any(|s| s["id"].as_str().unwrap().ends_with("example.budget()"))
    );
    let called = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["caller"].as_str().unwrap().ends_with("example.budget()"))
        .unwrap();
    assert!(called["target"].is_null());
    assert!(called["level"].is_null());
}

/// `programs` are the program entry points the pack declares (spec #31), as identities relative
/// to the repository, sorted; a pack with none passes an empty list.
fn conform(fixture: &str, symbols: &[&str], programs: &[&str], calls: &[ExpectedCall]) {
    let repo = fixture_repo(&format!("conformance_{fixture}"), fixture, Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let id = |rel: &str| format!("{REPO}::{rel}");

    let actual: Vec<&str> = json["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    let mut expected: Vec<String> = symbols.iter().map(|s| id(s)).collect();
    expected.sort();
    assert_eq!(actual, expected, "definitions of {fixture}");

    let actual: Vec<&str> = (json["processes"].as_array().unwrap().iter())
        .filter(|p| p["kind"] == "program")
        .map(|p| p["entry_point"].as_str().unwrap())
        .collect();
    let expected: Vec<String> = programs.iter().map(|s| id(s)).collect();
    assert_eq!(actual, expected, "program entry points of {fixture}");

    for (call, level, targets) in calls {
        let site = json["calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == id(call))
            .unwrap_or_else(|| panic!("no call {call} in {fixture}"));
        let mut found: Vec<&str> = site["target"].as_str().into_iter().collect();
        if let Some(candidates) = site["candidates"].as_array() {
            found.extend(candidates.iter().map(|c| c.as_str().unwrap()));
        }
        let wanted: Vec<String> = targets.iter().map(|t| id(t)).collect();
        assert_eq!(
            (site["level"].as_str(), found),
            (*level, wanted.iter().map(String::as_str).collect()),
            "call {call} in {fixture}"
        );
    }

    assert_golden(&format!("{fixture}.export.json"), &export);
}

#[test]
fn typescript_and_javascript() {
    conform(
        "conformance-ts-js",
        &[
            "src/app.ts::main",
            "src/legacy.cjs::legacy",
            "src/math.ts::add",
            "src/math.ts::double",
            "src/math.ts::scale",
            "src/math.ts::scale.times",
            "src/math.ts::square",
            "src/messages/consume.ts::consumeOrders",
            "src/messages/consume.ts::consumeOrders.kafka orders.created orders.shipped",
            "src/messages/consume.ts::consumePayments",
            "src/messages/consume.ts::consumeRefunds",
            "src/messages/consume.ts::consumeRefunds.kafka refunds",
            "src/messages/consume.ts::listen",
            "src/messages/consume.ts::listenRetries",
            "src/messages/consume.ts::record",
            "src/messages/exchanges.ts::announce",
            "src/messages/exchanges.ts::bind",
            "src/messages/exchanges.ts::listen",
            "src/messages/exchanges.ts::onAlert",
            "src/messages/exchanges.ts::onAudit",
            "src/messages/exchanges.ts::onShipment",
            "src/messages/handlers.ts::onPayment",
            "src/messages/bullmq.ts::enqueue",
            "src/messages/bullmq.ts::enqueueAll",
            "src/messages/bullmq.ts::enqueueTwice",
            "src/messages/bullmq.ts::notBullmq",
            "src/messages/bullmq.ts::record",
            "src/messages/bullmq.ts::runJob",
            "src/messages/bullmq.ts::startDynamic",
            "src/messages/bullmq.ts::startJobs",
            "src/messages/bullmq.ts::startMail",
            "src/messages/bullmq.ts::startMail.bullmq mail-eu",
            "src/messages/redis.ts::append",
            "src/messages/redis.ts::appendBilling",
            "src/messages/redis.ts::appendDynamic",
            "src/messages/redis.ts::appendIoredis",
            "src/messages/redis.ts::appendNodeRedis",
            "src/messages/redis.ts::notRedis",
            "src/messages/redis.ts::read",
            "src/messages/redis.ts::readBilling",
            "src/messages/redis.ts::readIoredis",
            "src/messages/redis.ts::readMany",
            "src/messages/redis.ts::readNamed",
            "src/messages/redis.ts::readNodeRedis",
            "src/messages/nats.ts::notPublished",
            "src/messages/nats.ts::onAudit",
            "src/messages/nats.ts::publishAll",
            "src/messages/nats.ts::record",
            "src/messages/nats.ts::subscribeMiddle",
            "src/messages/nats.ts::subscribeOne",
            "src/messages/nats.ts::subscribeRest",
            "src/messages/nats.ts::subscribeRest.nats jobs.>",
            "src/messages/sqs.ts::consumeAudit",
            "src/messages/sqs.ts::consumeMail",
            "src/messages/sqs.ts::consumeMail.sqs mail",
            "src/messages/sqs.ts::enqueue",
            "src/messages/sqs.ts::enqueueBilling",
            "src/messages/sqs.ts::notQueued",
            "src/messages/sqs.ts::onAudit",
            "src/messages/sqs.ts::receiveBilling",
            "src/messages/sqs.ts::receiveJobs",
            "src/messages/sqs.ts::record",
            "src/messages/sqs.ts::sendJob",
            "src/messages/sqs.ts::sendThroughNamespace",
            "src/messages/produce.ts::deliver",
            "src/messages/produce.ts::emit",
            "src/messages/produce.ts::emitRefund",
            "src/messages/produce.ts::notProduced",
            "src/messages/produce.ts::produceAll",
            "src/messages/produce.ts::relay",
            "src/messages/produce.ts::retry",
            "src/messages/queues.ts::enqueue",
            "src/messages/queues.ts::enqueueReports",
            "src/messages/queues.ts::queue",
            "src/messages/queues.ts::record",
            "src/messages/queues.ts::work",
            "src/messages/queues.ts::work.amqp tasks",
            "src/messages/queues.ts::workReports",
            "src/shadow.ts::again",
            "src/shadow.ts::caught",
            "src/shadow.ts::helper",
            "src/shadow.ts::hoisted",
            "src/shadow.ts::listed",
            "src/shadow.ts::looped",
            "src/shadow.ts::nested",
            "src/shadow.ts::plain",
            "src/shadow.ts::rebound",
            "src/shadow.ts::twice",
            "src/shadow.ts::unparenthesized",
            "src/shapes/circle.ts::Circle",
            "src/shapes/circle.ts::Circle.area",
            "src/shapes/circle.ts::Circle.constructor",
            "src/shapes/shape.ts::Shape",
            "src/shapes/shape.ts::Shape.area",
            "src/shapes/shape.ts::Shape.describe",
            "src/shapes/shape.ts::Shape.unit",
            "src/util/format.mjs::format",
            "src/util/log.js::default",
            "src/view.tsx::render",
            "src/widget.jsx::render",
        ],
        // TS/JS declares no program entry points.
        &[],
        &[
            // In-file scope: top level, nested function, `this.method` inside a class.
            ("src/math.ts::double::add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/math.ts::scale::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            (
                "src/math.ts::scale::times#0",
                EXACT,
                &["src/math.ts::scale.times"],
            ),
            (
                "src/shapes/shape.ts::Shape.describe::this.area#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.area"],
            ),
            // Imports: named through `../` without extension, named with alias, default.
            (
                "src/shapes/circle.ts::Circle.area::square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            ("src/app.ts::main::plus#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::double#0",
                EXACT,
                &["src/math.ts::double"],
            ),
            ("src/app.ts::main::scale#0", EXACT, &["src/math.ts::scale"]),
            // A namespace import whose `.js` extension names the `.ts` file.
            (
                "src/app.ts::main::numbers.square#0",
                EXACT,
                &["src/math.ts::square"],
            ),
            // Re-exports through `index` resolution: a namespace, a named one with its
            // extension, a default renamed.
            ("src/app.ts::main::math.add#0", EXACT, &["src/math.ts::add"]),
            (
                "src/app.ts::main::util.format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            (
                "src/app.ts::main::log#0",
                EXACT,
                &["src/util/log.js::default"],
            ),
            // The `@/*` alias of `tsconfig.json`, to an `index` file re-exporting a class, then
            // its static method; and from a TSX file.
            (
                "src/app.ts::main::Shape.unit#0",
                EXACT,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/view.tsx::render::format#0",
                EXACT,
                &["src/util/format.mjs::format"],
            ),
            // A package outside the repository stays unresolved.
            ("src/app.ts::main::useState#0", None, &[]),
            // Only a unique name match finds these: an inherited static method, a method on
            // a parameter, a function no import names.
            (
                "src/app.ts::main::Circle.unit#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.unit"],
            ),
            (
                "src/app.ts::main::shape.describe#0",
                INFERRED,
                &["src/shapes/shape.ts::Shape.describe"],
            ),
            (
                "src/app.ts::main::legacy#0",
                INFERRED,
                &["src/legacy.cjs::legacy"],
            ),
            // Several definitions fit: all are listed.
            (
                "src/app.ts::main::shape.area#0",
                AMBIGUOUS,
                &[
                    "src/shapes/circle.ts::Circle.area",
                    "src/shapes/shape.ts::Shape.area",
                ],
            ),
            (
                "src/app.ts::main::render#0",
                AMBIGUOUS,
                &["src/view.tsx::render", "src/widget.jsx::render"],
            ),
            // No definition has the name.
            ("src/util/log.js::default::console.log#0", None, &[]),
            // Hidden names: a `catch`, `for ... of`, `let` or `var` binding, an arrow parameter
            // without parentheses, a nested or array parameter pattern, and one spelled like a
            // constant or a parameter of the same function hide the top-level `helper`.
            (
                "src/shadow.ts::plain::helper#0",
                EXACT,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::caught::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::looped::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::rebound::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::hoisted::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::unparenthesized::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::nested::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::listed::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::twice::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
            (
                "src/shadow.ts::again::helper#0",
                INFERRED,
                &["src/shadow.ts::helper"],
            ),
        ],
    );
}

#[test]
fn python() {
    conform(
        "conformance-python",
        &[
            "clients/defaults.py::call_keyword_only",
            "clients/defaults.py::call_with_splats",
            "clients/defaults.py::create_default",
            "clients/defaults.py::keyword_only",
            "clients/defaults.py::list_defaults",
            "clients/defaults.py::other_base",
            "clients/defaults.py::override_keyword_only",
            "clients/defaults.py::request",
            "clients/defaults.py::with_splats",
            "clients/dynamic.py::build_url",
            "clients/dynamic.py::computed",
            "clients/dynamic.py::formatted",
            "clients/dynamic.py::from_environment",
            "clients/dynamic.py::percent",
            "clients/dynamic.py::unknown_method",
            "clients/environment.py::client_base_url",
            "clients/environment.py::computed_name",
            "clients/environment.py::default_argument",
            "clients/environment.py::default_in_environ_get",
            "clients/environment.py::default_reads_another",
            "clients/environment.py::environ_get",
            "clients/environment.py::fallback",
            "clients/environment.py::getenv_call",
            "clients/environment.py::imported_getenv",
            "clients/environment.py::imported_subscript",
            "clients/environment.py::interpolated",
            "clients/environment.py::named",
            "clients/environment.py::string_methods",
            "clients/environment.py::subscript",
            "clients/environment.py::two_variables",
            "clients/modules.py::create_user",
            "clients/modules.py::list_orders",
            "clients/modules.py::list_users",
            "clients/modules.py::read_order",
            "clients/modules.py::lookup",
            "clients/modules.py::purge",
            "clients/modules.py::rename_user",
            "clients/modules.py::status",
            "clients/sessions.py::Cache",
            "clients/sessions.py::Cache.get",
            "clients/sessions.py::Gateway",
            "clients/sessions.py::Gateway.__init__",
            "clients/sessions.py::Gateway.health",
            "clients/sessions.py::annotated",
            "clients/sessions.py::bound_by_async_with",
            "clients/sessions.py::bound_by_with",
            "clients/sessions.py::from_module",
            "clients/sessions.py::from_scope",
            "clients/sessions.py::hand_over",
            "clients/sessions.py::no_base",
            "clients/sessions.py::passed",
            "clients/sessions.py::with_base",
            "clients/wrappers.py::Api",
            "clients/wrappers.py::Api.__init__",
            "clients/wrappers.py::Api.profile",
            "clients/wrappers.py::Api.user",
            "clients/wrappers.py::backoff",
            "clients/wrappers.py::drop_order",
            "clients/wrappers.py::get",
            "clients/wrappers.py::Pages",
            "clients/wrappers.py::Pages.__init__",
            "clients/wrappers.py::Pages.load",
            "clients/wrappers.py::Pages.load_shared",
            "clients/wrappers.py::class_method",
            "clients/wrappers.py::explicit_instance",
            "clients/wrappers.py::Inventory",
            "clients/wrappers.py::Inventory.__init__",
            "clients/wrappers.py::Inventory.fetch_item",
            "clients/wrappers.py::not_a_client",
            "clients/wrappers.py::Reports",
            "clients/wrappers.py::Reports.pull",
            "clients/wrappers.py::unrelated",
            "clients/wrappers.py::stock",
            "clients/wrappers.py::list_by_keyword",
            "clients/wrappers.py::list_orders",
            "clients/wrappers.py::list_patiently",
            "clients/wrappers.py::remove",
            "clients/wrappers.py::retry",
            "clients/wrappers.py::send",
            "families.py::F",
            "families.py::E",
            "families.py::E.pick",
            "families.py::D",
            "families.py::D.pick",
            "families.py::C",
            "families.py::B",
            "families.py::A",
            "families.py::A.go",
            "families.py::Tangled",
            "families.py::Tangled.go",
            "families.py::Stored",
            "families.py::Stored.save",
            "families.py::Model",
            "families.py::Model.go",
            "families.py::Client",
            "families.py::Remote",
            "families.py::Remote.go",
            "families.py::Plain",
            "families.py::Plain.keep",
            "families.py::Plain.go",
            "families.py::Lone",
            "families.py::Lone.go",
            "families.py::Lone.make",
            "receivers.py::Engine",
            "receivers.py::Engine.start",
            "receivers.py::Untyped",
            "receivers.py::Untyped.__init__",
            "receivers.py::Untyped.run",
            "receivers.py::Typed",
            "receivers.py::Typed.__init__",
            "receivers.py::Typed.run",
            "receivers.py::Runner",
            "receivers.py::Runner.stride",
            "receivers.py::Runner.each",
            "receivers.py::module_read",
            "receivers.py::with_as",
            "receivers.py::comprehension",
            "receivers.py::walrus",
            "receivers.py::except_as",
            "receivers.py::lambdas",
            "receivers.py::param_reassigned",
            "receivers.py::swap",
            "receivers.py::spare_read",
            "receivers.py::fallback",
            "receivers.py::lacking",
            "receivers.py::optional",
            "reports/__init__.py::build",
            "cycles.py::Ping",
            "cycles.py::Ping.go",
            "cycles.py::Pong",
            "cycles.py::Pong.ring",
            "cycles.py::Knot",
            "cycles.py::Knot.go",
            "cycles.py::Loop",
            "cycles.py::Loop.Inner",
            "cycles.py::Deep",
            "cycles.py::Deep.go",
            "cycles.py::crossed",
            "cycle_b.py::use",
            "bindings.py::Line",
            "bindings.py::Line.dial",
            "bindings.py::Unpacked",
            "bindings.py::Unpacked.__init__",
            "bindings.py::Unpacked.call",
            "bindings.py::Looped",
            "bindings.py::Looped.__init__",
            "bindings.py::Looped.call",
            "bindings.py::Opened",
            "bindings.py::Opened.__init__",
            "bindings.py::Opened.call",
            "bindings.py::Reset",
            "bindings.py::Reset.call",
            "bindings.py::Reset.call.reset",
            "bindings.py::Outside",
            "bindings.py::Outside.call",
            "bindings.py::poke",
            "bindings.py::nested",
            "bindings.py::nested.mid",
            "bindings.py::nested.mid.inner",
            "bindings.py::factory",
            "bindings.py::factory.Inner",
            "bindings.py::factory.Inner.reset",
            "bindings.py::matched",
            "bindings.py::imported",
            "bindings.py::lazy",
            "bindings.py::Left",
            "bindings.py::Left.left",
            "bindings.py::Right",
            "bindings.py::Right.right",
            "bindings.py::Both",
            "bindings.py::Both.go",
            "bindings.py::ObjectLeft",
            "bindings.py::ObjectRight",
            "bindings.py::ObjectRight.side",
            "bindings.py::ObjectBoth",
            "bindings.py::ObjectBoth.go",
            "bindings.py::mixin",
            "bindings.py::Store",
            "bindings.py::Store.stash",
            "bindings.py::Mixed",
            "bindings.py::Mixed.go",
            "defs.py::Registry",
            "defs.py::Registry.Meta",
            "defs.py::Registry.Meta.label",
            "defs.py::Registry.add",
            "defs.py::Registry.build",
            "defs.py::Registry.load",
            "defs.py::Registry.tagged",
            "defs.py::Registry.unit",
            "defs.py::decorated",
            "defs.py::fetch_all",
            "defs.py::outer",
            "defs.py::outer.inner",
            "defs.py::plain",
            "defs.py::register",
            "defs.py::register.wrapper",
            "defs.py::sorted_names",
            "explicit/core.py::default",
            "explicit/early.py::kept",
            "explicit/early.py::swept",
            "explicit/early.py::taken",
            "explicit/first.py::pick",
            "explicit/letters.py::alpha",
            "explicit/letters.py::beta",
            "explicit/mod.py::_hidden",
            "explicit/mod.py::outside",
            "explicit/mod.py::public",
            "explicit/order.py::order",
            "explicit/second.py::pick",
            "explicit/sweeper.py::swept",
            "explicit/use.py::report",
            "explicit/util.py::get",
            "geometry/extra/measure.py::perimeter",
            "geometry/shapes.py::Circle",
            "geometry/shapes.py::Circle.__init__",
            "geometry/shapes.py::Circle.area",
            "geometry/shapes.py::Circle.describe",
            "geometry/shapes.py::Shape",
            "geometry/shapes.py::Shape.area",
            "geometry/shapes.py::Shape.unit",
            "geometry/shapes.py::area_of",
            "geometry/shapes.py::scale",
            "geometry/units.py::to_cm",
            "helpers.py::clamp",
            "helpers.py::format_name",
            "jobs/internal.py::_helper",
            "jobs/internal.py::public",
            "jobs/listed.py::left_out",
            "jobs/listed.py::listed",
            "jobs/single.py::__main__",
            "jobs/steps.py::fetch",
            "jobs/steps.py::store",
            "jobs/sync.py::__main__",
            "jobs/sync.py::run",
            "legacy.py::legacy",
            "main.py::main",
            "messages/confluent.py::Bus",
            "messages/confluent.py::Bus.relay",
            "messages/confluent.py::Counter",
            "messages/confluent.py::Counter.produce",
            "messages/confluent.py::consume_orders",
            "messages/confluent.py::consume_pattern",
            "messages/confluent.py::consume_refunds",
            "messages/confluent.py::emit",
            "messages/confluent.py::emit_refund",
            "messages/confluent.py::not_produced",
            "messages/confluent.py::produce_all",
            "messages/confluent.py::produce_unknown",
            "messages/confluent.py::relay_dynamic",
            "messages/kafka_python.py::Relay",
            "messages/kafka_python.py::Relay.__init__",
            "messages/kafka_python.py::Relay.forward",
            "messages/kafka_python.py::idle",
            "messages/kafka_python.py::listen_alias",
            "messages/kafka_python.py::listen_invoices",
            "messages/kafka_python.py::listen_retries",
            "messages/kafka_python.py::not_sent",
            "messages/kafka_python.py::send_invoice",
            "messages/kafka_python.py::send_retry",
            "messages/nats_py.py::Plain",
            "messages/nats_py.py::Plain.publish",
            "messages/nats_py.py::announce",
            "messages/nats_py.py::announce_all",
            "messages/nats_py.py::bus",
            "messages/nats_py.py::not_published",
            "messages/nats_py.py::on_job",
            "messages/nats_py.py::publish_all",
            "messages/nats_py.py::publish_legacy",
            "messages/nats_py.py::publish_returned",
            "messages/nats_py.py::publish_unknown",
            "messages/nats_py.py::record",
            "messages/nats_py.py::subscribe_middle",
            "messages/nats_py.py::subscribe_one",
            "messages/nats_py.py::subscribe_rest",
            "messages/pika_amqp.py::Worker",
            "messages/pika_amqp.py::Worker.__init__",
            "messages/pika_amqp.py::Worker.consume",
            "messages/pika_amqp.py::Worker.enqueue",
            "messages/pika_amqp.py::Worker.on_report",
            "messages/pika_amqp.py::announce",
            "messages/pika_amqp.py::bind",
            "messages/pika_amqp.py::enqueue",
            "messages/pika_amqp.py::enqueue_reports",
            "messages/pika_amqp.py::enqueue_unknown",
            "messages/pika_amqp.py::listen",
            "messages/pika_amqp.py::not_published",
            "messages/pika_amqp.py::on_alert",
            "messages/pika_amqp.py::on_audit",
            "messages/pika_amqp.py::on_mail",
            "messages/pika_amqp.py::on_task",
            "messages/pika_amqp.py::queue",
            "messages/pika_amqp.py::record",
            "messages/pika_amqp.py::send_chained",
            "messages/pika_amqp.py::send_mail",
            "messages/redis_streams.py::append",
            "messages/redis_streams.py::append_all",
            "messages/redis_streams.py::append_audit",
            "messages/redis_streams.py::append_more",
            "messages/redis_streams.py::append_unknown",
            "messages/redis_streams.py::not_streamed",
            "messages/redis_streams.py::read_many",
            "messages/redis_streams.py::read_orders",
            "messages/redis_streams.py::read_plain",
            "messages/redis_streams.py::read_unknown",
            "messages/shadowed.py::rebound_by_except",
            "messages/shadowed.py::rebound_by_loop",
            "messages/shadowed.py::rebound_by_parameter",
            "messages/shadowed.py::rebound_by_with",
            "messages/shadowed.py::rebound_constructor",
            "messages/sqs_boto.py::enqueue",
            "messages/sqs_boto.py::enqueue_billing",
            "messages/sqs_boto.py::not_queued",
            "messages/sqs_boto.py::receive_billing",
            "messages/sqs_boto.py::receive_jobs",
            "messages/sqs_boto.py::receive_mail",
            "messages/sqs_boto.py::receive_session",
            "messages/sqs_boto.py::receive_unknown",
            "messages/sqs_boto.py::send_batch",
            "messages/sqs_boto.py::send_chained",
            "messages/sqs_boto.py::send_joined",
            "messages/sqs_boto.py::send_job",
            "messages/sqs_boto.py::send_session",
            "methods.py::Base",
            "methods.py::Base.ping",
            "methods.py::Base.shared",
            "methods.py::Dial",
            "methods.py::Dial.spin",
            "methods.py::Leaf",
            "methods.py::Leaf.run",
            "methods.py::Left",
            "methods.py::Mixin",
            "methods.py::Mixin.mixed",
            "methods.py::Mixin.shared",
            "methods.py::Panel",
            "methods.py::Panel.__init__",
            "methods.py::Panel.build",
            "methods.py::Panel.check",
            "methods.py::Panel.create",
            "methods.py::Panel.draw",
            "methods.py::Panel.draw.later",
            "methods.py::Panel.render",
            "methods.py::Right",
            "methods.py::Right.hook",
            "methods.py::Root",
            "methods.py::Root.hook",
            "methods.py::make",
            "methods.py::module_level",
            "methods.py::outside",
            "methods.py::shadowed",
            "methods.py::use",
            "rebound.py::deleted",
            "rebound.py::grown",
            "rebound.py::nested",
            "rebound.py::nested.inner",
            "rebound.py::reach",
            "rebound.py::replaced",
            "scopes.py::Helper",
            "scopes.py::Outer",
            "scopes.py::Outer.Helper",
            "scopes.py::Outer.m",
            "shadow.py::from_module",
            "shadow/__init__.py::from_package",
            "shop/catalog/urls.py::products",
            "shop/catalog/urls.py::upload",
            "shop/orders/items.py::stock",
            "shop/orders/views.py::OrderList",
            "shop/orders/views.py::OrderList.get",
            "shop/orders/views.py::OrderList.post",
            "shop/orders/views.py::OrderList.total",
            "shop/orders/views.py::order",
            "shop/views.py::about",
            "shop/views.py::archive",
            "shop/views.py::contact",
            "shop/views.py::health",
            "shop/views.py::legal",
            "shop/views.py::vintage",
            "src/billing/invoice.py::total",
            "src/billing/tax.py::fee",
            "src/billing/tax.py::vat",
            "stars/computed.py::named",
            "stars/computed.py::unnamed",
            "stars/grown.py::grown",
            "stars/grown.py::seed",
            "stars/literal.py::Shown",
            "stars/literal.py::listed",
            "stars/literal.py::unlisted",
            "stars/underscore.py::_internal",
            "stars/underscore.py::visible",
            "web/accounts.py::update",
            "web/api.py::echo",
            "web/api.py::health",
            "web/api.py::ping",
            "web/factory.py::build_admin",
            "web/factory.py::build_admin.remove_user",
            "web/factory.py::mount",
            "web/factory.py::relay",
            "web/items.py::create_item",
            "web/items.py::read_item",
            "web/partial.py::build",
            "web/partial.py::build.items",
            "web/partial.py::read_part",
            "web/partial.py::read_user",
            "web/pages.py::daily",
            "web/pages.py::fetch",
            "web/pages.py::index",
            "web/pages.py::order",
            "web/pages.py::shortcut",
        ],
        // The main guards of `jobs/` (both spellings, neither of the lookalikes) and the
        // console scripts of `pyproject.toml` that name a function of a file (`ghost.module`
        // and `jobs.sync:missing` name none).
        &[
            "jobs/single.py::__main__",
            "jobs/sync.py::__main__",
            "jobs/sync.py::run",
            "reports/__init__.py::build",
        ],
        &[
            // In-file scope: a comprehension and a nested function, a nested function called
            // from its parent, a class body, methods, and lambdas, whose calls belong to the
            // enclosing symbol (the module, here, or `sorted_names`).
            ("defs.py::fetch_all::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::outer.inner::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::outer::inner#0", EXACT, &["defs.py::outer.inner"]),
            ("defs.py::decorated::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::Registry::plain#0", EXACT, &["defs.py::plain"]),
            (
                "defs.py::Registry.load::fetch_all#0",
                EXACT,
                &["defs.py::fetch_all"],
            ),
            (
                "defs.py::Registry.unit::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            (
                "defs.py::Registry.tagged::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            (
                "defs.py::Registry.Meta.label::plain#0",
                EXACT,
                &["defs.py::plain"],
            ),
            ("defs.py::plain#0", EXACT, &["defs.py::plain"]),
            ("defs.py::sorted_names::plain#0", EXACT, &["defs.py::plain"]),
            // Built-ins, a decorator from an imported package, parameters and `self`: no
            // definition fits.
            ("defs.py::register::functools.wraps#0", None, &[]),
            ("defs.py::register.wrapper::func#0", None, &[]),
            ("defs.py::Registry.add::self.items.append#0", None, &[]),
            ("defs.py::Registry.build::cls#0", None, &[]),
            ("defs.py::sorted_names::sorted#0", None, &[]),
            // Relative imports: `from .m import f`, and `from . import m` and `from .. import
            // m`, where `m` is a module of the package.
            (
                "geometry/shapes.py::scale::to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/shapes.py::Circle.area::scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            (
                "geometry/shapes.py::Circle.describe::to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/extra/measure.py::perimeter::units.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "geometry/extra/measure.py::perimeter::area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            // `import a.b` binds `a.b` as a whole.
            (
                "geometry/extra/measure.py::perimeter::geometry.shapes.scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            // The `src/` layout, with `from . import tax` and `from .tax import vat`.
            (
                "src/billing/invoice.py::total::vat#0",
                EXACT,
                &["src/billing/tax.py::vat"],
            ),
            (
                "src/billing/invoice.py::total::tax.fee#0",
                EXACT,
                &["src/billing/tax.py::fee"],
            ),
            // Imports through a package: a re-export of a class and of a function, the
            // function as an attribute of the package, a submodule `__init__.py` imports, a
            // deeper module named in full, and an alias of a module.
            (
                "main.py::main::Round#0",
                EXACT,
                &["geometry/shapes.py::Circle"],
            ),
            (
                "main.py::main::area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            (
                "main.py::main::geometry.area_of#0",
                EXACT,
                &["geometry/shapes.py::area_of"],
            ),
            (
                "main.py::main::geometry.units.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            (
                "main.py::main::geometry.extra.measure.perimeter#0",
                EXACT,
                &["geometry/extra/measure.py::perimeter"],
            ),
            (
                "main.py::main::gu.to_cm#0",
                EXACT,
                &["geometry/units.py::to_cm"],
            ),
            // `from a.b import c as d`, a package re-export from the `src/` layout and a
            // module alias in it.
            (
                "main.py::main::grow#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            (
                "main.py::main::total#0",
                EXACT,
                &["src/billing/invoice.py::total"],
            ),
            (
                "main.py::main::tax.vat#0",
                EXACT,
                &["src/billing/tax.py::vat"],
            ),
            // `import *` brings in the exports: all public names, only `__all__` when it is
            // a literal, and every public name when it is computed. What it leaves out is
            // found by name alone.
            (
                "main.py::main::format_name#0",
                EXACT,
                &["helpers.py::format_name"],
            ),
            ("main.py::main::clamp#0", EXACT, &["helpers.py::clamp"]),
            (
                "main.py::main::listed#0",
                EXACT,
                &["stars/literal.py::listed"],
            ),
            (
                "main.py::main::Shown#0",
                EXACT,
                &["stars/literal.py::Shown"],
            ),
            (
                "main.py::main::unlisted#0",
                INFERRED,
                &["stars/literal.py::unlisted"],
            ),
            (
                "main.py::main::visible#0",
                EXACT,
                &["stars/underscore.py::visible"],
            ),
            (
                "main.py::main::_internal#0",
                INFERRED,
                &["stars/underscore.py::_internal"],
            ),
            (
                "main.py::main::named#0",
                EXACT,
                &["stars/computed.py::named"],
            ),
            (
                "main.py::main::unnamed#0",
                EXACT,
                &["stars/computed.py::unnamed"],
            ),
            // An explicit import or an attribute of a module sees every name, whatever
            // `__all__` and the underscore prefix say, and a re-export `__all__` leaves out; a
            // star import in a package is found by name too, `default` included.
            (
                "explicit/use.py::report::outside#0",
                EXACT,
                &["explicit/mod.py::outside"],
            ),
            (
                "explicit/use.py::report::_hidden#0",
                EXACT,
                &["explicit/mod.py::_hidden"],
            ),
            (
                "explicit/use.py::report::m.outside#0",
                EXACT,
                &["explicit/mod.py::outside"],
            ),
            (
                "explicit/use.py::report::m._hidden#0",
                EXACT,
                &["explicit/mod.py::_hidden"],
            ),
            (
                "explicit/use.py::report::beta#0",
                EXACT,
                &["explicit/letters.py::beta"],
            ),
            (
                "explicit/use.py::report::default#0",
                EXACT,
                &["explicit/core.py::default"],
            ),
            // Of two imports of one name the later wins, and an explicit import of a package
            // outside the repository is not overridden by a star import.
            (
                "explicit/use.py::report::pick#0",
                EXACT,
                &["explicit/second.py::pick"],
            ),
            ("explicit/use.py::report::get#0", None, &[]),
            // The latest import that binds a name decides: an external one after a resolving
            // one leaves it unresolved, a star import after an explicit one takes the name
            // over when it provides it, and only then.
            ("explicit/order.py::order::taken#0", None, &[]),
            (
                "explicit/order.py::order::swept#0",
                EXACT,
                &["explicit/sweeper.py::swept"],
            ),
            (
                "explicit/order.py::order::kept#0",
                EXACT,
                &["explicit/early.py::kept"],
            ),
            // A package comes before a module of the same name.
            (
                "explicit/use.py::report::from_package#0",
                EXACT,
                &["shadow/__init__.py::from_package"],
            ),
            (
                "explicit/use.py::report::shadow.from_package#0",
                EXACT,
                &["shadow/__init__.py::from_package"],
            ),
            // A package outside the repository stays unresolved.
            ("main.py::main::np.array#0", None, &[]),
            // Only a unique name match finds a function no import names.
            ("main.py::main::legacy#0", INFERRED, &["legacy.py::legacy"]),
            // A local assigned from a constructor call, here through an aliased import.
            (
                "main.py::main::circle.describe#0",
                EXACT,
                &["geometry/shapes.py::Circle.describe"],
            ),
            // `self` and `cls` reach the class, then its bases in the order written, in
            // Python's method resolution order, also from a nested function.
            (
                "methods.py::Panel.draw::self.ping#0",
                EXACT,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::Panel.draw::self.shared#0",
                EXACT,
                &["methods.py::Base.shared"],
            ),
            (
                "methods.py::Panel.draw::self.mixed#0",
                EXACT,
                &["methods.py::Mixin.mixed"],
            ),
            (
                "methods.py::Panel.draw::self.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::Panel.draw.later::self.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::Panel.build::cls.create#0",
                EXACT,
                &["methods.py::Panel.create"],
            ),
            (
                "methods.py::Leaf.run::self.hook#0",
                EXACT,
                &["methods.py::Right.hook"],
            ),
            // Attribute annotations, in a method and in the class body.
            (
                "methods.py::Panel.draw::self.shape.area#0",
                EXACT,
                &["geometry/shapes.py::Shape.area"],
            ),
            (
                "methods.py::Panel.draw::self.frame.describe#0",
                EXACT,
                &["geometry/shapes.py::Circle.describe"],
            ),
            // Parameter annotations: a name, `Optional[...]` and a string; a constructor
            // call in the same scope, and at the module level.
            (
                "methods.py::use::panel.draw#0",
                EXACT,
                &["methods.py::Panel.draw"],
            ),
            (
                "methods.py::use::maybe.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::ahead.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::built.ping#0",
                EXACT,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::module_level::current.render#0",
                EXACT,
                &["methods.py::Panel.render"],
            ),
            // By name: a method the class family lacks, a static method's parameter, what a
            // function returns, and a local that shadows a typed module-level name.
            (
                "methods.py::Panel.draw::self.spin#0",
                INFERRED,
                &["methods.py::Dial.spin"],
            ),
            (
                "methods.py::Panel.check::value.ping#0",
                INFERRED,
                &["methods.py::Base.ping"],
            ),
            (
                "methods.py::use::made.render#0",
                INFERRED,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::shadowed::current.render#0",
                INFERRED,
                &["methods.py::Panel.render"],
            ),
            (
                "methods.py::use::made.area#0",
                AMBIGUOUS,
                &[
                    "geometry/shapes.py::Circle.area",
                    "geometry/shapes.py::Shape.area",
                ],
            ),
            // A receiver whose class, or whose value's callee, is outside the repository:
            // what its methods are is not known, and no name match is tried.
            ("methods.py::outside::session.render#0", None, &[]),
            ("methods.py::outside::client.render#0", None, &[]),
            ("methods.py::outside::log.render#0", None, &[]),
            // Python's method resolution order (C3), a hierarchy it rejects, bases outside the
            // repository that come before the class defining a method, and `object`.
            (
                "families.py::A.go::self.pick#0",
                EXACT,
                &["families.py::D.pick"],
            ),
            (
                "families.py::Tangled.go::self.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            (
                "families.py::Model.go::self.save#0",
                INFERRED,
                &["families.py::Stored.save"],
            ),
            (
                "families.py::Remote.go::self.save#0",
                INFERRED,
                &["families.py::Stored.save"],
            ),
            (
                "families.py::Plain.keep::self.go#0",
                EXACT,
                &["families.py::Plain.go"],
            ),
            // `self` and `cls` whose family lacks the method: a name match.
            (
                "families.py::Lone.go::self.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            (
                "families.py::Lone.make::cls.pick#0",
                AMBIGUOUS,
                &["families.py::D.pick", "families.py::E.pick"],
            ),
            // Bindings that hide an outer one: `self.x = ...` without an annotation, a loop
            // variable named `this`, `with`, comprehensions, `:=`, `except`, lambdas, a
            // reassigned parameter, `global` and an import's fallback; an annotation stays
            // stronger, and a single module-level binding is read from a function.
            (
                "receivers.py::Untyped.run::self.engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::Typed.run::self.engine.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::Runner.each::this.stride#0",
                INFERRED,
                &["receivers.py::Runner.stride"],
            ),
            (
                "receivers.py::module_read::engine.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::with_as::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::comprehension::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::walrus::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::except_as::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::lambdas::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::param_reassigned::engine.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::spare_read::spare.start#0",
                INFERRED,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::fallback::Circle#0",
                INFERRED,
                &["geometry/shapes.py::Circle"],
            ),
            // An annotated receiver whose family lacks the method, and `X | None` both ways.
            (
                "receivers.py::lacking::engine.stride#0",
                INFERRED,
                &["receivers.py::Runner.stride"],
            ),
            (
                "receivers.py::optional::first.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            (
                "receivers.py::optional::second.start#0",
                EXACT,
                &["receivers.py::Engine.start"],
            ),
            // Bases and annotations that resolve through themselves, in one module or two.
            (
                "cycles.py::Ping.go::self.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::Knot.go::self.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::Deep.go::self.x.y.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::crossed::a.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycles.py::crossed::b.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            (
                "cycle_b.py::use::other.ring#0",
                INFERRED,
                &["cycles.py::Pong.ring"],
            ),
            // A receiver attribute bound in a compound target or from a closure, a `nonlocal` that
            // skips a class scope, a `case` capture and a function-local import hide an outer
            // binding; a write from outside the class is not seen; a function-local import
            // still resolves.
            (
                "bindings.py::Unpacked.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Looped.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Opened.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Reset.call::self.line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::Outside.call::self.line.dial#0",
                EXACT,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::nested::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::factory::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::matched::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::imported::line.dial#0",
                INFERRED,
                &["bindings.py::Line.dial"],
            ),
            (
                "bindings.py::lazy::scale#0",
                EXACT,
                &["geometry/shapes.py::scale"],
            ),
            // One outside base shared by two classes (`ABC`, `object`) has one place in C3; a base
            // that is not a name hides what comes after it.
            (
                "bindings.py::Both.go::self.right#0",
                EXACT,
                &["bindings.py::Right.right"],
            ),
            (
                "bindings.py::ObjectBoth.go::self.side#0",
                EXACT,
                &["bindings.py::ObjectRight.side"],
            ),
            (
                "bindings.py::Mixed.go::self.stash#0",
                INFERRED,
                &["bindings.py::Store.stash"],
            ),
            // The last top-level binding of a name decides: an assignment or a `del` after a
            // definition leaves a name match alone, and a star import of a module whose
            // `__all__` grows by `+=` takes what it adds.
            (
                "rebound.py::reach::replaced#0",
                INFERRED,
                &["rebound.py::replaced"],
            ),
            (
                "rebound.py::reach::deleted#0",
                INFERRED,
                &["rebound.py::deleted"],
            ),
            (
                "rebound.py::reach::grown#0",
                EXACT,
                &["stars/grown.py::grown"],
            ),
            // So does the last binding in a function: a nested definition rebound later is not
            // what its name reaches, and no name match finds a nested one.
            ("rebound.py::nested::inner#0", None, &[]),
            // A class body's names are in scope for its own statements, not for its methods.
            (
                "scopes.py::Outer::Helper#0",
                EXACT,
                &["scopes.py::Outer.Helper"],
            ),
            (
                "scopes.py::Outer.m::Helper#0",
                EXACT,
                &["scopes.py::Helper"],
            ),
            // Several definitions fit: a method on a parameter.
            (
                "geometry/shapes.py::area_of::shape.area#0",
                AMBIGUOUS,
                &[
                    "geometry/shapes.py::Circle.area",
                    "geometry/shapes.py::Shape.area",
                ],
            ),
        ],
    );
}

#[test]
fn rust() {
    const MAIN: &str = "api/src/main.rs";
    conform(
        "conformance-rust",
        &[
            "crates/actix/src/admin.rs::extra",
            "crates/actix/src/admin.rs::extras",
            "crates/actix/src/admin.rs::more",
            "crates/actix/src/lib.rs::macros",
            "crates/actix/src/macros.rs::conditional",
            "crates/actix/src/macros.rs::listed",
            "crates/actix/src/macros.rs::scoped",
            "crates/actix/src/macros.rs::via_cfg_if",
            "crates/actix/src/macros.rs::via_services",
            "crates/actix/src/macros.rs::via_vec",
            "crates/greeter/src/main.rs::index",
            "crates/greeter/src/main.rs::names",
            "crates/greeter/src/main.rs::rocket",
            "crates/greeter/src/names.rs::named",
            "crates/rocket/src/hiding.rs::rocket",
            "crates/rocket/src/hiding.rs::rocket.Builder",
            "crates/rocket/src/hiding.rs::rocket.Builder.mount",
            "crates/rocket/src/hiding.rs::rocket.build",
            "crates/rocket/src/hiding.rs::serve",
            "crates/rocket/src/joined.rs::documented",
            "crates/rocket/src/joined.rs::joined_a",
            "crates/rocket/src/joined.rs::joined_b",
            "crates/rocket/src/lib.rs::hiding",
            "crates/rocket/src/lib.rs::joined",
            "crates/rocket/src/stray.rs::local",
            "crates/rocket/src/stray.rs::three",
            "crates/actix/src/accounts.rs::api",
            "crates/actix/src/accounts.rs::create",
            "crates/actix/src/accounts.rs::files",
            "crates/actix/src/accounts.rs::list",
            "crates/actix/src/accounts.rs::remove",
            "crates/actix/src/accounts.rs::show",
            "crates/actix/src/accounts.rs::sync",
            "crates/actix/src/admin.rs::audit",
            "crates/actix/src/admin.rs::dashboard",
            "crates/actix/src/admin.rs::prefix",
            "crates/actix/src/admin.rs::routes",
            "crates/actix/src/admin.rs::status",
            "crates/actix/src/admin.rs::tools",
            "crates/actix/src/admin.rs::unreadable",
            "crates/actix/src/lib.rs::accounts",
            "crates/actix/src/lib.rs::admin",
            "crates/actix/src/lib.rs::orders",
            "crates/actix/src/lib.rs::serve_catalog",
            "crates/actix/src/lib.rs::serve_catalog.PUT /inline",
            "crates/actix/src/lib.rs::stray",
            "crates/actix/src/orders.rs::collection",
            "crates/actix/src/orders.rs::create",
            "crates/actix/src/orders.rs::echo",
            "crates/actix/src/orders.rs::item",
            "crates/actix/src/orders.rs::item.DELETE /orders/{}",
            "crates/actix/src/orders.rs::list",
            "crates/actix/src/orders.rs::ping",
            "crates/actix/src/orders.rs::show",
            "crates/actix/src/orders.rs::update",
            "crates/actix/src/stray.rs::Bounds",
            "crates/actix/src/stray.rs::binding",
            "crates/actix/src/stray.rs::bound",
            "crates/actix/src/stray.rs::early",
            "crates/actix/src/stray.rs::early_then_bound",
            "crates/actix/src/stray.rs::Holder",
            "crates/flat/lib.rs::pages",
            "crates/flat/lib.rs::serve",
            "crates/flat/pages.rs::page",
            "crates/rocket/src/stray.rs::elsewhere",
            "crates/actix/src/stray.rs::elsewhere",
            "crates/actix/src/stray.rs::elsewhere.get",
            "crates/actix/src/stray.rs::held",
            "crates/actix/src/stray.rs::lonely",
            "crates/actix/src/stray.rs::plain",
            "crates/actix/src/stray.rs::plug",
            "crates/actix/src/stray.rs::rebind",
            "crates/actix/src/stray.rs::shadowed",
            "crates/rocket/src/lib.rs::pages",
            "crates/rocket/src/lib.rs::posts",
            "crates/rocket/src/lib.rs::rocket",
            "crates/rocket/src/lib.rs::stray",
            "crates/rocket/src/pages.rs::about",
            "crates/rocket/src/pages.rs::index",
            "crates/rocket/src/posts.rs::create",
            "crates/rocket/src/posts.rs::files",
            "crates/rocket/src/posts.rs::legacy",
            "crates/rocket/src/posts.rs::list",
            "crates/rocket/src/posts.rs::old",
            "crates/rocket/src/posts.rs::search",
            "crates/rocket/src/posts.rs::show",
            "crates/rocket/src/stray.rs::base",
            "crates/rocket/src/stray.rs::lonely",
            "crates/rocket/src/stray.rs::moved",
            "api/extra/legacy_util.rs::Store",
            "api/extra/legacy_util.rs::Store.new",
            "api/extra/legacy_util.rs::legacy",
            "api/src/bin/sync/main.rs::main",
            "api/src/bin/sync/main.rs::step",
            "api/src/bin/sync/step.rs::tick",
            "api/src/bin/tool.rs::helper",
            "api/src/bin/tool.rs::helper.go",
            "api/src/bin/tool.rs::main",
            "api/src/main.rs::below",
            "api/src/main.rs::below.go",
            "api/src/main.rs::elsewhere",
            "api/src/main.rs::extras",
            "api/src/main.rs::gated",
            "api/src/main.rs::imp",
            "api/src/main.rs::mock",
            "api/src/main.rs::native",
            "api/src/main.rs::real",
            "api/src/mock.rs::client",
            "api/src/native.rs::counter",
            "api/src/real.rs::client",
            "api/src/gated.rs::connect",
            "api/src/gated.rs::outside_tests",
            "api/src/gated.rs::Gauge",
            "api/src/gated.rs::Gauge.plain",
            "api/src/gated.rs::Gauge.unix_method",
            "api/src/gated.rs::always",
            "api/src/gated.rs::on_linux",
            "api/src/gated.rs::through_net",
            "api/src/gated.rs::tests",
            "api/src/gated.rs::tests.helper",
            "api/src/gated.rs::unix_without_metrics",
            "api/src/main.rs::hidden",
            "api/src/main.rs::isolated",
            "api/src/main.rs::isolated.far",
            "api/src/main.rs::isolated.go",
            "api/src/main.rs::launch",
            "api/src/main.rs::local_wins",
            "api/src/main.rs::local_wins.compute",
            "api/src/main.rs::local_wins.go",
            "api/src/main.rs::with_param",
            "api/src/launch.rs::launch",
            "crates/ledger-core/src/lib.rs::meter",
            "crates/ledger-core/src/meter.rs::meter",
            "crates/scratch/src/lib.rs::reach",
            "api/src/main.rs::loops",
            "api/src/main.rs::loops.f",
            "api/src/main.rs::outside",
            "api/src/main.rs::prelude",
            "api/src/main.rs::ring",
            "api/src/main.rs::scoped",
            "api/src/main.rs::scoped.go",
            "api/src/main.rs::within",
            "api/src/extras.rs::dup",
            "api/src/prelude.rs::dup",
            "api/src/prelude.rs::greet",
            "api/src/ring.rs::c",
            "api/src/ring.rs::d",
            "api/src/ring.rs::e",
            "api/src/ring.rs::x",
            "api/src/ring.rs::y",
            "api/src/ring/c.rs::Gear",
            "api/src/ring/c.rs::Gear.spin",
            "api/src/ring/c.rs::hidden",
            "api/src/ring/c.rs::internal",
            "api/src/store.rs::private_helper",
            "crates/billing/src/billing.rs::settle",
            "crates/billing/src/billing.rs::copies",
            "crates/ledger-core/src/lib.rs::Ledger",
            "crates/ledger-core/src/lib.rs::Ledger.Clone.clone",
            "crates/ledger-core/src/lib.rs::amounts",
            "crates/ledger-core/src/lib.rs::audit",
            "crates/ledger-core/src/lib.rs::entries",
            "crates/ledger-core/src/lib.rs::entries.add",
            "crates/ledger-core/src/lib.rs::entries.seal",
            "crates/ledger-core/src/lib.rs::internal",
            "crates/ledger-core/src/lib.rs::internal.secret",
            "crates/ledger-core/src/lib.rs::post",
            "api/src/main.rs::checks",
            "api/src/main.rs::checks.compute",
            "api/src/main.rs::checks.run_checks",
            "api/src/main.rs::checks.verify",
            "api/src/main.rs::compute",
            "api/src/main.rs::inner",
            "api/src/main.rs::inner.helper",
            "api/src/main.rs::inner.twice",
            "api/src/main.rs::main",
            "api/src/main.rs::methods",
            "api/src/methods.rs::Counter",
            "api/src/methods.rs::Counter.Probe.probe",
            "api/src/methods.rs::Counter.bump",
            "api/src/methods.rs::Counter.new",
            "api/src/methods.rs::Counter.read",
            "api/src/methods.rs::Counter.reset",
            "api/src/methods.rs::Counter.twice",
            "api/src/methods.rs::Dash",
            "api/src/methods.rs::Dash.mark",
            "api/src/methods.rs::Dash.tick",
            "api/src/methods.rs::Gauge",
            "api/src/methods.rs::Gauge.Probe.probe",
            "api/src/methods.rs::Gauge.bump",
            "api/src/methods.rs::Gauge.insert",
            "api/src/methods.rs::Gauge.new",
            "api/src/methods.rs::Gauge.read",
            "api/src/methods.rs::Probe",
            "api/src/methods.rs::Probe.probe",
            "api/src/methods.rs::Probe.summary",
            "api/src/methods.rs::annotated",
            "api/src/methods.rs::blocks",
            "api/src/methods.rs::bounded",
            "api/src/methods.rs::built",
            "api/src/methods.rs::dynamic",
            "api/src/methods.rs::exclusive",
            "api/src/methods.rs::fresh",
            "api/src/methods.rs::generic",
            "api/src/methods.rs::named",
            "api/src/methods.rs::opaque_impl",
            "api/src/methods.rs::outside",
            "api/src/methods.rs::pick",
            "api/src/methods.rs::probed",
            "api/src/methods.rs::retyped",
            "api/src/methods.rs::shadowed",
            "api/src/methods.rs::shared",
            "api/src/methods.rs::Build",
            "api/src/methods.rs::Build.make",
            "api/src/methods.rs::Build.run",
            "api/src/methods.rs::Build.seed",
            "api/src/methods.rs::Counter.Build.run",
            "api/src/methods.rs::Counter.Build.seed",
            "api/src/methods.rs::wrap",
            "api/src/methods.rs::opened",
            "api/src/methods.rs::modular",
            "api/src/methods.rs::reopened",
            "api/src/methods.rs::built_by_default",
            "api/src/methods.rs::wrapped",
            "api/src/methods.rs::sliced",
            "api/src/methods.rs::cleared",
            "api/src/methods.rs::Config",
            "api/src/methods.rs::Config.Clone.clone",
            "api/src/methods.rs::Config.Default.default",
            "api/src/methods.rs::Plain",
            "api/src/methods.rs::derived",
            "api/src/methods.rs::marked",
            "api/src/methods.rs::marked_impl",
            "api/src/store.rs::Persist.persist",
            "api/src/main.rs::nested",
            "api/src/main.rs::nested.deep",
            "api/src/main.rs::nested.plain",
            "api/src/main.rs::routes",
            "api/src/main.rs::run",
            "api/src/main.rs::start",
            "api/src/main.rs::store",
            "api/src/main.rs::util",
            "api/src/nested/deep.rs::dive",
            "api/src/nested/plain.rs::walk",
            "api/src/routes/health.rs::check",
            "api/src/routes/mod.rs::health",
            "api/src/routes/mod.rs::serve",
            "api/src/store.rs::Mode",
            "api/src/store.rs::Named",
            "api/src/store.rs::Named.label",
            "api/src/store.rs::Persist",
            "api/src/store.rs::Persist.describe",
            "api/src/store.rs::Store",
            "api/src/store.rs::Store.From#2.from",
            "api/src/store.rs::Store.From.from",
            "api/src/store.rs::Store.Persist.persist",
            "api/src/store.rs::Store.empty",
            "api/src/store.rs::Store.new",
            "api/src/store.rs::Store.save",
            "api/src/store.rs::Tagged",
            "api/src/store.rs::Tagged.label",
            "api/src/store.rs::disk",
            "api/src/store.rs::reset",
            "api/src/store.rs::tests",
            "api/src/store.rs::tests.t",
            "api/src/store/disk.rs::Store.check",
            "api/src/store/disk.rs::Store.reload",
            "api/src/store/disk.rs::open",
            "api/tools/admin.rs::main",
            "api/tools/admin.rs::shared",
            "api/tools/shared.rs::ping",
            "crates/billing/src/billing.rs::invoice",
            "crates/billing/src/billing.rs::total",
            "crates/billing/src/billing.rs::util",
            "crates/billing/src/billing.rs::util.round",
            "crates/billing/src/invoice.rs::Invoice",
            "crates/billing/src/invoice.rs::Invoice.amount",
            "crates/billing/src/invoice.rs::Invoice.new",
            "crates/billing/src/invoice.rs::tax",
            // A package the workspace's `exclude` lists is still a crate of the repository.
            "crates/scratch/src/lib.rs::ignored",
            "crates/worker/src/agents.rs::agents",
            "crates/worker/src/agents.rs::module_functions",
            "crates/worker/src/clients.rs::blocking",
            "crates/worker/src/clients.rs::built",
            "crates/worker/src/clients.rs::inline",
            "crates/worker/src/clients.rs::requested",
            "crates/worker/src/clients.rs::verbs",
            "crates/worker/src/derived.rs::ApiExt",
            "crates/worker/src/derived.rs::ApiExt.api",
            "crates/worker/src/derived.rs::Client.ApiExt.api",
            "crates/worker/src/derived.rs::Factory",
            "crates/worker/src/derived.rs::Factory.fetch_named",
            "crates/worker/src/derived.rs::Factory.make",
            "crates/worker/src/derived.rs::Sender",
            "crates/worker/src/derived.rs::Sender.send_named",
            "crates/worker/src/derived.rs::by_name",
            "crates/worker/src/derived.rs::derived",
            "crates/worker/src/derived.rs::send_named",
            "crates/worker/src/derived.rs::shadowed",
            "crates/worker/src/lib.rs::derived",
            "crates/worker/src/wrappers.rs::recurse",
            "crates/worker/src/wrappers.rs::recursive",
            "crates/worker/src/dynamic.rs::looped",
            "crates/worker/src/dynamic.rs::pick",
            "crates/worker/src/dynamic.rs::spec",
            "crates/worker/src/dynamic.rs::unresolved",
            "crates/worker/src/env.rs::dotenv",
            "crates/worker/src/env.rs::fallbacks",
            "crates/worker/src/env.rs::full_path",
            "crates/worker/src/env.rs::function_import",
            "crates/worker/src/env.rs::imported",
            "crates/worker/src/env.rs::macros",
            "crates/worker/src/env.rs::mid_url",
            "crates/worker/src/env.rs::optional",
            "crates/worker/src/env.rs::two_reads",
            "crates/worker/src/lib.rs::agents",
            "crates/worker/src/lib.rs::clients",
            "crates/worker/src/lib.rs::dynamic",
            "crates/worker/src/lib.rs::env",
            "crates/worker/src/lib.rs::modules",
            "crates/worker/src/lib.rs::not_env",
            "crates/worker/src/lib.rs::settings",
            "crates/worker/src/lib.rs::urls",
            "crates/worker/src/lib.rs::wrappers",
            "crates/worker/src/modules.rs::Cache",
            "crates/worker/src/modules.rs::Cache.get",
            "crates/worker/src/modules.rs::blocking_health",
            "crates/worker/src/modules.rs::health",
            "crates/worker/src/modules.rs::items",
            "crates/worker/src/modules.rs::lookalikes",
            "crates/worker/src/not_env.rs::env",
            "crates/worker/src/not_env.rs::env.var",
            "crates/worker/src/not_env.rs::foreign",
            "crates/worker/src/not_env.rs::foreign.read",
            "crates/worker/src/not_env.rs::hidden",
            "crates/worker/src/not_env.rs::hidden.closure",
            "crates/worker/src/not_env.rs::hidden.parameter",
            "crates/worker/src/not_env.rs::other",
            "crates/worker/src/not_env.rs::other.bare",
            "crates/worker/src/not_env.rs::own_module",
            "crates/worker/src/urls.rs::consts",
            "crates/worker/src/urls.rs::formats",
            "crates/worker/src/urls.rs::strings",
            "crates/worker/src/wrappers.rs::Api",
            "crates/worker/src/wrappers.rs::Api.ping",
            "crates/worker/src/wrappers.rs::send",
            "crates/worker/src/wrappers.rs::through_a_parameter",
            "crates/worker/src/wrappers.rs::api_get",
            "crates/worker/src/wrappers.rs::open_ended",
            "crates/worker/src/wrappers.rs::resource",
            "crates/worker/src/wrappers.rs::wrapped",
            "crates/worker/src/lib.rs::kafka",
            "crates/worker/src/lib.rs::mq_shadowed",
            "crates/worker/src/lib.rs::queues",
            "crates/worker/src/lib.rs::rabbit",
            "crates/worker/src/lib.rs::streams",
            "crates/worker/src/lib.rs::subjects",
            "crates/worker/src/kafka.rs::make_producer",
            "crates/worker/src/kafka.rs::produce_all",
            "crates/worker/src/kafka.rs::base_produce",
            "crates/worker/src/kafka.rs::emit",
            "crates/worker/src/kafka.rs::emit_refund",
            "crates/worker/src/kafka.rs::produce_unknown",
            "crates/worker/src/kafka.rs::consume_orders",
            "crates/worker/src/kafka.rs::consume_refunds",
            "crates/worker/src/kafka.rs::consume_pattern",
            "crates/worker/src/kafka.rs::Transport",
            "crates/worker/src/kafka.rs::Transport.send",
            "crates/worker/src/kafka.rs::not_rdkafka",
            "crates/worker/src/rabbit.rs::publish_all",
            "crates/worker/src/rabbit.rs::bind_all",
            "crates/worker/src/rabbit.rs::work_jobs",
            "crates/worker/src/rabbit.rs::audit",
            "crates/worker/src/rabbit.rs::publish_unknown",
            "crates/worker/src/queues.rs::sqs_client",
            "crates/worker/src/queues.rs::send_all",
            "crates/worker/src/queues.rs::enqueue",
            "crates/worker/src/queues.rs::enqueue_audit",
            "crates/worker/src/queues.rs::send_prefixed",
            "crates/worker/src/queues.rs::send_unknown",
            "crates/worker/src/queues.rs::receive_jobs",
            "crates/worker/src/queues.rs::receive_many",
            "crates/worker/src/queues.rs::other_calls",
            "crates/worker/src/queues.rs::Mailer",
            "crates/worker/src/queues.rs::Mailer.send_message",
            "crates/worker/src/queues.rs::Mailer.queue_url",
            "crates/worker/src/queues.rs::not_sqs",
            "crates/worker/src/subjects.rs::publish_all",
            "crates/worker/src/subjects.rs::publish_with_options",
            "crates/worker/src/subjects.rs::listen_orders",
            "crates/worker/src/subjects.rs::listen_queue",
            "crates/worker/src/subjects.rs::not_nats",
            "crates/worker/src/subjects.rs::Bus",
            "crates/worker/src/subjects.rs::Bus.publish",
            "crates/worker/src/subjects.rs::Bus.subscribe",
            "crates/worker/src/streams.rs::append_all",
            "crates/worker/src/streams.rs::read_group",
            "crates/worker/src/streams.rs::read_inline",
            "crates/worker/src/streams.rs::read_unknown",
            "crates/worker/src/streams.rs::read_plain",
            "crates/worker/src/streams.rs::hidden_names",
            "crates/worker/src/streams.rs::hidden_names.nested",
            "crates/worker/src/streams.rs::reassigned",
            "crates/worker/src/streams.rs::Log",
            "crates/worker/src/streams.rs::Log.xadd",
            "crates/worker/src/streams.rs::not_redis",
            "crates/worker/src/mq_shadowed.rs::shadowed",
            "crates/worker/src/mq_shadowed.rs::Outbox",
            "crates/worker/src/mq_shadowed.rs::Outbox.send",
            "crates/worker/src/mq_shadowed.rs::unreached",
            "crates/web/src/admin.rs::mount",
            "crates/web/src/admin.rs::reset",
            "crates/web/src/admin.rs::router",
            "crates/web/src/admin.rs::stats",
            "crates/web/src/files.rs::read",
            "crates/web/src/files.rs::router",
            "crates/web/src/health.rs::check",
            "crates/web/src/health.rs::echo",
            "crates/web/src/items.rs::create",
            "crates/web/src/items.rs::list",
            "crates/web/src/items.rs::remove",
            "crates/web/src/items.rs::router",
            "crates/web/src/items.rs::show",
            "crates/web/src/items.rs::update",
            "crates/web/src/lib.rs::Holder",
            "crates/web/src/lib.rs::Holder.mount",
            "crates/web/src/lib.rs::admin",
            "crates/web/src/lib.rs::app",
            "crates/web/src/lib.rs::app.GET /api/ping",
            "crates/web/src/lib.rs::files",
            "crates/web/src/lib.rs::health",
            "crates/web/src/lib.rs::items",
            "crates/web/src/lib.rs::legacy",
            "crates/web/src/lib.rs::legacy.router",
            "crates/web/src/lib.rs::Builder",
            "crates/web/src/lib.rs::Builder.routes",
            "crates/web/src/lib.rs::hop",
            "crates/web/src/lib.rs::reassigned",
            "crates/web/src/lib.rs::rebuilt",
            "crates/web/src/lib.rs::reassigned_items",
            "crates/web/src/lib.rs::reassigned_items.router",
            "crates/web/src/lib.rs::renamed",
            "crates/web/src/lib.rs::renamed.router",
            "crates/web/src/lib.rs::served",
            "crates/web/src/lib.rs::services",
            "crates/web/src/lib.rs::services.fallback",
            "crates/web/src/lib.rs::services.router",
            "crates/web/src/lib.rs::services.wrapped",
            "crates/web/src/lib.rs::wrapper",
            "crates/web/src/lib.rs::late",
            "crates/web/src/lib.rs::late.router",
            "crates/web/src/lib.rs::local",
            "crates/web/src/lib.rs::orphans",
            "crates/web/src/lib.rs::orphans.router",
            "crates/web/src/lib.rs::prefix",
            "crates/web/src/lib.rs::unread",
            "crates/web/src/local.rs::Router",
            "crates/web/src/local.rs::Router.get",
            "crates/web/src/local.rs::wire",
        ],
        // `fn main` of each binary target: `src/main.rs`, `src/bin/`, a `[[bin]] path`.
        &[
            "api/src/bin/sync/main.rs::main",
            "api/src/bin/tool.rs::main",
            "api/src/main.rs::main",
            "api/tools/admin.rs::main",
        ],
        &[
            // An inline module, by its path and through `self::`.
            (
                "api/src/main.rs::inner.twice::helper#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            (
                "api/src/main.rs::inner.twice::mod.inner.helper#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            (
                "api/src/main.rs::main::inner.helper#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            // `super::` out of an inline module is read from the file's top, past the
            // module's own `compute`.
            // `#[cfg(test)] use crate::mock::client;` binds nothing: the production client.
            (
                "api/src/gated.rs::connect::client#0",
                EXACT,
                &["api/src/real.rs::client"],
            ),
            (
                "api/src/main.rs::checks.verify::mod.start#0",
                EXACT,
                &["api/src/main.rs::start"],
            ),
            (
                "api/src/main.rs::checks.run_checks::mod.compute#0",
                EXACT,
                &["api/src/main.rs::compute"],
            ),
            (
                "api/src/main.rs::checks.run_checks::compute#0",
                EXACT,
                &["api/src/main.rs::checks.compute"],
            ),
            // `<name>.rs`, `mod.rs` and `#[path]` modules, those declared inside an inline
            // module, and `crate::`; a `let` of a module's name does not hide it.
            (
                "api/src/main.rs::main::store.Store.new#0",
                EXACT,
                &["api/src/store.rs::Store.new"],
            ),
            (
                "api/src/main.rs::main::routes.serve#0",
                EXACT,
                &["api/src/routes/mod.rs::serve"],
            ),
            (
                "api/src/main.rs::main::util.legacy#0",
                EXACT,
                &["api/extra/legacy_util.rs::legacy"],
            ),
            (
                "api/src/main.rs::main::crate.store.reset#0",
                EXACT,
                &["api/src/store.rs::reset"],
            ),
            (
                "api/src/main.rs::main::nested.deep.dive#0",
                EXACT,
                &["api/src/nested/deep.rs::dive"],
            ),
            (
                "api/src/main.rs::main::nested.plain.walk#0",
                EXACT,
                &["api/src/nested/plain.rs::walk"],
            ),
            // A closure's call belongs to the function around it; the closure's binding hides
            // the function of its name.
            (
                "api/src/main.rs::main::start#0",
                EXACT,
                &["api/src/main.rs::start"],
            ),
            (
                "api/src/main.rs::main::run#0",
                INFERRED,
                &["api/src/main.rs::run"],
            ),
            // Paths into crates outside the repository.
            ("api/src/main.rs::main::String.new#0", None, &[]),
            ("api/src/main.rs::main::std.mem.drop#0", None, &[]),
            (
                "api/src/routes/mod.rs::serve::health.check#0",
                EXACT,
                &["api/src/routes/health.rs::check"],
            ),
            (
                "api/src/routes/mod.rs::serve::super.compute#0",
                EXACT,
                &["api/src/main.rs::compute"],
            ),
            (
                "api/src/routes/mod.rs::serve::store.save#0",
                EXACT,
                &["api/src/store.rs::Store.save"],
            ),
            (
                "api/src/routes/health.rs::check::crate.inner.helper#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            (
                "api/src/routes/health.rs::check::super.super.compute#0",
                EXACT,
                &["api/src/main.rs::compute"],
            ),
            // `Self::`, `Type::` and `Trait::` calls, inherent and trait `impl`s, and an
            // `impl` in another file than the type.
            (
                "api/src/store.rs::Store.new::Self.empty#0",
                EXACT,
                &["api/src/store.rs::Store.empty"],
            ),
            (
                "api/src/store.rs::Store.empty::disk.open#0",
                EXACT,
                &["api/src/store/disk.rs::open"],
            ),
            (
                "api/src/store.rs::Store.empty::Store.reload#0",
                EXACT,
                &["api/src/store/disk.rs::Store.reload"],
            ),
            (
                "api/src/store.rs::Store.Persist.persist::Self.describe#0",
                EXACT,
                &["api/src/store.rs::Persist.describe"],
            ),
            (
                "api/src/store.rs::reset::Store.new#0",
                EXACT,
                &["api/src/store.rs::Store.new"],
            ),
            (
                "api/src/store.rs::reset::Persist.describe#0",
                EXACT,
                &["api/src/store.rs::Persist.describe"],
            ),
            // Two traits' default bodies, two blocks of one trait: each one fits.
            (
                "api/src/store.rs::reset::Store.label#0",
                AMBIGUOUS,
                &[
                    "api/src/store.rs::Named.label",
                    "api/src/store.rs::Tagged.label",
                ],
            ),
            (
                "api/src/store.rs::reset::Store.from#0",
                AMBIGUOUS,
                &[
                    "api/src/store.rs::Store.From#2.from",
                    "api/src/store.rs::Store.From.from",
                ],
            ),
            // An inline module does not see the file's own `Store`: `use crate::util::Store`
            // names another one, which a `use super::*` glob does not override; the glob takes
            // the file's private items.
            (
                "api/src/store.rs::tests.t::Store.new#0",
                EXACT,
                &["api/extra/legacy_util.rs::Store.new"],
            ),
            (
                "api/src/store.rs::tests.t::private_helper#0",
                EXACT,
                &["api/src/store.rs::private_helper"],
            ),
            (
                "api/src/store/disk.rs::Store.reload::open#0",
                EXACT,
                &["api/src/store/disk.rs::open"],
            ),
            (
                "api/src/store/disk.rs::Store.reload::Self.check#0",
                EXACT,
                &["api/src/store/disk.rs::Store.check"],
            ),
            // Crate roots from `src/bin/`, `src/bin/<name>/main.rs` and `[[bin]]`.
            (
                "api/src/bin/tool.rs::main::helper.go#0",
                EXACT,
                &["api/src/bin/tool.rs::helper.go"],
            ),
            (
                "api/src/bin/sync/main.rs::main::step.tick#0",
                EXACT,
                &["api/src/bin/sync/step.rs::tick"],
            ),
            (
                "api/tools/admin.rs::main::shared.ping#0",
                EXACT,
                &["api/tools/shared.rs::ping"],
            ),
            // The second member, its root named by `[lib] path`.
            (
                "crates/billing/src/billing.rs::total::invoice.Invoice.new#0",
                EXACT,
                &["crates/billing/src/invoice.rs::Invoice.new"],
            ),
            (
                "crates/billing/src/billing.rs::total::invoice.tax#0",
                EXACT,
                &["crates/billing/src/invoice.rs::tax"],
            ),
            (
                "crates/billing/src/billing.rs::total::(call_expression).amount#0",
                INFERRED,
                &["crates/billing/src/invoice.rs::Invoice.amount"],
            ),
            // A name match keeps to the crates the caller reaches: its own and the libraries it
            // depends on, never `api`, which depends on it, nor `billing` from `ledger-core`.
            (
                "crates/billing/src/billing.rs::copies::l.clone#0",
                INFERRED,
                &["crates/ledger-core/src/lib.rs::Ledger.Clone.clone"],
            ),
            (
                "crates/ledger-core/src/lib.rs::amounts::i.amount#0",
                None,
                &[],
            ),
            // `use crate::util as u` renames a module written in place.
            (
                "crates/billing/src/invoice.rs::tax::u.round#0",
                EXACT,
                &["crates/billing/src/billing.rs::util.round"],
            ),
            // The other workspace member, by its name with the hyphen turned into an
            // underscore, through a `use` of its module.
            (
                "crates/billing/src/billing.rs::settle::ledger_core.post#0",
                EXACT,
                &["crates/ledger-core/src/lib.rs::post"],
            ),
            (
                "crates/billing/src/billing.rs::settle::entries.add#0",
                EXACT,
                &["crates/ledger-core/src/lib.rs::entries.add"],
            ),
            // `use`s and `pub use`s, from the first member into the others.
            (
                "api/src/main.rs::main::aid#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            (
                "api/src/main.rs::main::dive#0",
                EXACT,
                &["api/src/nested/deep.rs::dive"],
            ),
            (
                "api/src/main.rs::main::Repo.new#0",
                EXACT,
                &["api/src/store.rs::Store.new"],
            ),
            (
                "api/src/main.rs::main::stores.reset#0",
                EXACT,
                &["api/src/store.rs::reset"],
            ),
            (
                "api/src/main.rs::main::total#0",
                EXACT,
                &["crates/billing/src/billing.rs::total"],
            ),
            (
                "api/src/main.rs::main::Bill.new#0",
                EXACT,
                &["crates/billing/src/invoice.rs::Invoice.new"],
            ),
            (
                "api/src/main.rs::main::billing.invoice.tax#0",
                EXACT,
                &["crates/billing/src/invoice.rs::tax"],
            ),
            (
                "api/src/main.rs::main::billing.Invoice.new#0",
                EXACT,
                &["crates/billing/src/invoice.rs::Invoice.new"],
            ),
            (
                "api/src/main.rs::main::ledger_core.post#0",
                EXACT,
                &["crates/ledger-core/src/lib.rs::post"],
            ),
            (
                "api/src/main.rs::main::books.add#0",
                EXACT,
                &["crates/ledger-core/src/lib.rs::entries.add"],
            ),
            // A crate outside the repository is external: `new` is not matched by name.
            ("api/src/main.rs::main::HashMap.new#0", None, &[]),
            // Globs: the public items of a module, not its private `dup`.
            (
                "api/src/main.rs::main::dup#0",
                EXACT,
                &["api/src/extras.rs::dup"],
            ),
            (
                "api/src/main.rs::main::greet#0",
                EXACT,
                &["api/src/prelude.rs::greet"],
            ),
            // `pub use` renames three files deep, and a cycle of re-exports.
            (
                "api/src/main.rs::main::ring.e.Wheel.spin#0",
                EXACT,
                &["api/src/ring/c.rs::Gear.spin"],
            ),
            (
                "api/src/main.rs::main::ring.Gear.spin#0",
                EXACT,
                &["api/src/ring/c.rs::Gear.spin"],
            ),
            ("api/src/main.rs::main::ring.x.Loop.unheard#0", None, &[]),
            // A `use` in a module binds there alone, hides the file's name of it and is not in
            // scope outside; `use super::*` takes the private `compute`.
            (
                "api/src/main.rs::scoped.go::aid#0",
                EXACT,
                &["api/src/main.rs::checks.run_checks"],
            ),
            (
                "api/src/main.rs::scoped.go::twice#0",
                EXACT,
                &["api/src/main.rs::inner.twice"],
            ),
            (
                "api/src/main.rs::outside::twice#0",
                INFERRED,
                &["api/src/main.rs::inner.twice"],
            ),
            (
                "api/src/main.rs::below.go::compute#0",
                EXACT,
                &["api/src/main.rs::compute"],
            ),
            // A function's `use` binds in it alone, and a `let` hides a `use`d name.
            (
                "api/src/main.rs::within::local_aid#0",
                EXACT,
                &["api/src/main.rs::inner.helper"],
            ),
            ("api/src/main.rs::elsewhere::local_aid#0", None, &[]),
            ("api/src/main.rs::loops.f::a#0", None, &[]),
            ("api/src/main.rs::hidden::aid#0", None, &[]),
            // A parameter hides the `use` that spells its name.
            ("api/src/main.rs::with_param::aid#0", None, &[]),
            // The file's `use`s and modules are not in scope in a module written in place: the
            // names are crates outside the file there, never name-matched.
            ("api/src/main.rs::isolated.go::aid#0", None, &[]),
            ("api/src/main.rs::isolated.far::inner.helper#0", None, &[]),
            (
                "api/src/main.rs::main::isolated.go#0",
                EXACT,
                &["api/src/main.rs::isolated.go"],
            ),
            (
                "api/src/main.rs::main::isolated.far#0",
                EXACT,
                &["api/src/main.rs::isolated.far"],
            ),
            // An item of the module beats what a glob gives.
            (
                "api/src/main.rs::local_wins.go::compute#0",
                EXACT,
                &["api/src/main.rs::local_wins.compute"],
            ),
            (
                "api/src/main.rs::main::local_wins.go#0",
                EXACT,
                &["api/src/main.rs::local_wins.go"],
            ),
            // `mod launch; use launch::launch;`: a module is no value.
            (
                "api/src/main.rs::main::launch#0",
                EXACT,
                &["api/src/launch.rs::launch"],
            ),
            (
                "api/src/main.rs::main::launch.launch#0",
                EXACT,
                &["api/src/launch.rs::launch"],
            ),
            // `mod meter; pub use meter::meter;` and a `pub use` in a `pub` module written in
            // place, called from the other crate.
            (
                "api/src/main.rs::main::ledger_core.meter#0",
                EXACT,
                &["crates/ledger-core/src/meter.rs::meter"],
            ),
            (
                "api/src/main.rs::main::ledger_core.entries.secret#0",
                EXACT,
                &["crates/ledger-core/src/lib.rs::internal.secret"],
            ),
            // `scratch` lists no dependency: `ledger_core` is outside the repository to it.
            (
                "crates/scratch/src/lib.rs::reach::ledger_core.post#0",
                None,
                &[],
            ),
            // Method calls: the receiver's type is written on a `let`, a parameter (`&T`, `&mut T`),
            // a field through `self`, or `self` itself; one fit is `exact`.
            (
                "api/src/methods.rs::annotated::c.bump#0",
                EXACT,
                &["api/src/methods.rs::Counter.bump"],
            ),
            (
                "api/src/methods.rs::shared::c.read#0",
                EXACT,
                &["api/src/methods.rs::Counter.read"],
            ),
            (
                "api/src/methods.rs::exclusive::g.bump#0",
                EXACT,
                &["api/src/methods.rs::Gauge.bump"],
            ),
            (
                "api/src/methods.rs::Dash.tick::self.counter.bump#0",
                EXACT,
                &["api/src/methods.rs::Counter.bump"],
            ),
            (
                "api/src/methods.rs::Dash.tick::self.gauge.bump#0",
                EXACT,
                &["api/src/methods.rs::Gauge.bump"],
            ),
            (
                "api/src/methods.rs::Dash.tick::self.counter.twice#0",
                EXACT,
                &["api/src/methods.rs::Counter.twice"],
            ),
            (
                "api/src/methods.rs::Counter.twice::self.bump#0",
                EXACT,
                &["api/src/methods.rs::Counter.bump"],
            ),
            (
                "api/src/methods.rs::Counter.twice::self.bump#1",
                EXACT,
                &["api/src/methods.rs::Counter.bump"],
            ),
            // `self` in a trait's block, and in the default body of a trait.
            (
                "api/src/methods.rs::Counter.Probe.probe::self.read#0",
                EXACT,
                &["api/src/methods.rs::Counter.read"],
            ),
            (
                "api/src/methods.rs::Probe.summary::self.probe#0",
                EXACT,
                &["api/src/methods.rs::Probe.probe"],
            ),
            // The type is the declared return type of the function the `let` calls, `Self` included.
            (
                "api/src/methods.rs::built::c.bump#0",
                EXACT,
                &["api/src/methods.rs::Counter.bump"],
            ),
            (
                "api/src/methods.rs::built::g.read#0",
                EXACT,
                &["api/src/methods.rs::Gauge.read"],
            ),
            // Methods of `impl Trait for Type` are found through the type, with the trait's defaults.
            (
                "api/src/methods.rs::probed::c.probe#0",
                EXACT,
                &["api/src/methods.rs::Counter.Probe.probe"],
            ),
            (
                "api/src/methods.rs::probed::c.summary#0",
                EXACT,
                &["api/src/methods.rs::Probe.summary"],
            ),
            // A `dyn Trait`, a bounded generic and an `impl Trait` stop at the trait's method: no
            // implementation is searched.
            (
                "api/src/methods.rs::dynamic::p.probe#0",
                EXACT,
                &["api/src/methods.rs::Probe.probe"],
            ),
            (
                "api/src/methods.rs::generic::p.probe#0",
                EXACT,
                &["api/src/methods.rs::Probe.probe"],
            ),
            (
                "api/src/methods.rs::bounded::p.summary#0",
                EXACT,
                &["api/src/methods.rs::Probe.summary"],
            ),
            (
                "api/src/methods.rs::opaque_impl::p.probe#0",
                EXACT,
                &["api/src/methods.rs::Probe.probe"],
            ),
            // The type is not written (a `for` binding): the call goes by name.
            (
                "api/src/methods.rs::named::c.reset#0",
                INFERRED,
                &["api/src/methods.rs::Counter.reset"],
            ),
            (
                "api/src/methods.rs::named::c.bump#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.bump",
                    "api/src/methods.rs::Gauge.bump",
                ],
            ),
            (
                "api/src/methods.rs::blocks::c.read#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.read",
                    "api/src/methods.rs::Gauge.read",
                ],
            ),
            // A type from outside the repository is opaque: `insert` is not `Gauge.insert`.
            ("api/src/methods.rs::outside::m.insert#0", None, &[]),
            ("api/src/methods.rs::outside::made.insert#0", None, &[]),
            (
                "api/src/methods.rs::Dash.mark::self.marks.insert#0",
                None,
                &[],
            ),
            (
                "api/src/methods.rs::Gauge.insert::self.marks.insert#0",
                None,
                &[],
            ),
            // A binding of the name that disagrees with the typed one leaves it to the name match.
            (
                "api/src/methods.rs::shadowed::c.bump#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.bump",
                    "api/src/methods.rs::Gauge.bump",
                ],
            ),
            (
                "api/src/methods.rs::retyped::c.bump#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.bump",
                    "api/src/methods.rs::Gauge.bump",
                ],
            ),
            // `Self` of a trait's default body is the type the call named, not the trait; a type
            // parameter as a result names no class; a parameter whose type names no item hides a `let`.
            (
                "api/src/methods.rs::Build.make::Self.seed#0",
                EXACT,
                &["api/src/methods.rs::Build.seed"],
            ),
            (
                "api/src/methods.rs::Counter.Build.seed::Counter.new#0",
                EXACT,
                &["api/src/methods.rs::Counter.new"],
            ),
            (
                "api/src/methods.rs::built_by_default::Counter.make#0",
                EXACT,
                &["api/src/methods.rs::Build.make"],
            ),
            (
                "api/src/methods.rs::built_by_default::c.run#0",
                EXACT,
                &["api/src/methods.rs::Counter.Build.run"],
            ),
            (
                "api/src/methods.rs::wrapped::w.probe#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.Probe.probe",
                    "api/src/methods.rs::Gauge.Probe.probe",
                    "api/src/methods.rs::Probe.probe",
                ],
            ),
            (
                "api/src/methods.rs::sliced::c.read#0",
                AMBIGUOUS,
                &[
                    "api/src/methods.rs::Counter.read",
                    "api/src/methods.rs::Gauge.read",
                ],
            ),
            // A type is read among types and modules: a parameter named like the module does not hide it.
            (
                "api/src/methods.rs::modular::store.save#0",
                EXACT,
                &["api/src/store.rs::Store.save"],
            ),
            (
                "api/src/methods.rs::reopened::client.save#0",
                EXACT,
                &["api/src/store.rs::Store.save"],
            ),
            (
                "api/src/methods.rs::reopened::again.save#0",
                EXACT,
                &["api/src/store.rs::Store.save"],
            ),
            // Marker traits (`Send`, `Sync`, `Unpin`, `Sized`) and lifetimes are not a second bound.
            (
                "api/src/methods.rs::marked::p.summary#0",
                EXACT,
                &["api/src/methods.rs::Probe.summary"],
            ),
            (
                "api/src/methods.rs::marked_impl::p.probe#0",
                EXACT,
                &["api/src/methods.rs::Probe.probe"],
            ),
            (
                "api/src/methods.rs::cleared::c.reset#0",
                EXACT,
                &["api/src/methods.rs::Counter.reset"],
            ),
            // A written type whose blocks lack the member: nothing, never another type's.
            ("api/src/methods.rs::derived::Plain.default#0", None, &[]),
            ("api/src/methods.rs::derived::q.clone#0", None, &[]),
            // `self` of a block of a type from outside the repository is opaque.
            (
                "crates/worker/src/derived.rs::Client.ApiExt.api::self.clone#0",
                None,
                &[],
            ),
            // A chain that starts at a call of an outside crate is that crate's, whatever
            // function of the file shares the crate's name.
            (
                "crates/greeter/src/main.rs::rocket::(call_expression).mount#0",
                None,
                &[],
            ),
            (
                "crates/flat/lib.rs::serve::(call_expression).mount#0",
                None,
                &[],
            ),
            // An attribute is no call of a function: neither by name nor through a path.
            ("crates/greeter/src/main.rs::get#0", None, &[]),
            ("crates/greeter/src/names.rs::get#0", None, &[]),
            ("crates/actix/src/stray.rs::elsewhere.get#0", None, &[]),
        ],
    );
    // The call inside `println!` is not indexed, and neither is a file no `mod` reaches.
    let repo = fixture_repo("conformance_rust_absent", "conformance-rust", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(!export.contains(&format!("{MAIN}::main::compute#")));
    assert!(!export.contains("orphan.rs"));
}

/// The Go pack (spec #25): functions, methods by their receiver's type, types and interface
/// methods; imports through the module path of `go.mod`, a `replace` and a nested module; one
/// scope per package directory; and build constraints for `linux/amd64`. A sibling repository of
/// the workspace provides the module its `require` of `example.com/acme/ledger` names.
#[test]
fn go() {
    const OUTER: &str = "scopes/scopes.go::outer";
    let ledger = Some("https://example.com/acme/ledger.git");
    fixture_repo(
        "conformance_conformance-go",
        "conformance-go-ledger",
        ledger,
    );
    conform(
        "conformance-go",
        &[
            "billing/invoice/invoice.go::Total",
            "billing/invoice/invoice.go::round",
            "client/fields.go::Billing",
            "client/fields.go::NewBilling",
            "client/fields.go::Billing.Items",
            "client/fields.go::Ledger",
            "client/fields.go::NewLedger",
            "client/fields.go::Ledger.Entries",
            "client/fields.go::Mirror",
            "client/fields.go::primary",
            "client/fields.go::secondary",
            "client/fields.go::Mirror.Sync",
            "client/fields.go::Proxy",
            "client/fields.go::NewProxy",
            "client/fields.go::plainProxy",
            "client/fields.go::Proxy.Forward",
            "client/fields.go::Relay",
            "client/fields.go::NewRelay",
            "client/fields.go::Relay.Swap",
            "client/fields.go::Relay.Send",
            "client/fields.go::Queue",
            "client/fields.go::NewQueue",
            "client/fields.go::Queue.Retarget",
            "client/fields.go::Queue.Push",
            "client/fields.go::Pair",
            "client/fields.go::NewPair",
            "client/fields.go::positional",
            "client/fields.go::Pair.Left",
            "client/fields.go::Pool",
            "client/fields.go::NewPool",
            "client/fields.go::pools",
            "client/fields.go::Pool.Lease",
            "client/fields.go::Unset",
            "client/fields.go::Unset.Idle",
            "client/env.go::FromEnv",
            "client/env.go::FromLookup",
            "client/flags.go::Configure",
            "client/net.go::Cache",
            "client/net.go::Cache.Get",
            "client/net.go::Clients",
            "client/net.go::Formats",
            "client/net.go::Lookup",
            "client/net.go::Package",
            "client/net.go::Requests",
            "client/net.go::Unresolved",
            "client/net.go::Urls",
            "client/net.go::Written",
            "client/net.go::init",
            "client/net.go::lookup",
            "client/rest.go::Factory",
            "client/rest.go::Gateway",
            "client/rest.go::Gateway.Items",
            "client/rest.go::Mixed",
            "client/rest.go::Other",
            "client/rest.go::Reassigned",
            "client/rest.go::Rest",
            "client/rest.go::Returned",
            "client/rest.go::Reused",
            "client/rest.go::Separate",
            "client/rest.go::Typed",
            "client/rest.go::Wrapper",
            "client/rest.go::fetchBase",
            "client/rest.go::fetchJSON",
            "client/rest.go::newClient",
            "client/rest.go::plainClient",
            "client/routes/routes.go::Move",
            "client/tenants.go::Rename",
            "client/wrappers.go::Cycle",
            "client/wrappers.go::Wrapped",
            "client/wrappers.go::get",
            "client/wrappers.go::getUser",
            "client/wrappers.go::ping",
            "client/wrappers.go::pong",
            "client/wrappers.go::send",
            "forks/billing/invoice/invoice.go::Total",
            "forks/billing/invoice/invoice.go::round",
            "internal/http-utils/utils.go::NewRequest",
            "internal/shapes/shapes.go::Circle",
            "internal/shapes/shapes.go::Circle.Area",
            "internal/shapes/shapes.go::Circle.Perimeter",
            "internal/shapes/shapes.go::Each",
            "internal/shapes/shapes.go::ID",
            "internal/shapes/shapes.go::Label",
            "internal/shapes/shapes.go::NewCircle",
            "internal/shapes/shapes.go::Shape",
            "internal/shapes/shapes.go::Shape.Area",
            "internal/shapes/shapes.go::Shape.Perimeter",
            "internal/shapes/shapes.go::clamp",
            "internal/units/units.go::Meters",
            "legacy/old/old.go::Old",
            "messages/confluent.go::ConfluentDeref",
            "messages/sarama.go::SaramaSend",
            "messages/segmentio.go::NewWriter",
            "messages/segmentio.go::ProduceLookalike",
            "messages/segmentio.go::Publisher",
            "messages/segmentio.go::Publisher.Send",
            "messages/segmentio.go::WriterConfig",
            "messages/shadowed.go::Service",
            "messages/shadowed.go::Service.Run",
            "messages/shadowed.go::ShadowedBlock",
            "messages/shadowed.go::ShadowedImport",
            "messages/shadowed.go::ShadowedRange",
            "messages/shadowed.go::Sink",
            "messages/shadowed.go::Sink.WriteMessages",
            "messages/confluent.go::ConfluentConsume",
            "messages/confluent.go::ConfluentPattern",
            "messages/confluent.go::ConfluentProduce",
            "messages/queues.go::SqsConsume",
            "messages/queues.go::SqsProduce",
            "messages/rabbit.go::Audit",
            "messages/rabbit.go::PublishAll",
            "messages/rabbit.go::PublishUnknown",
            "messages/rabbit.go::WorkJobs",
            "messages/rabbit.go::channel",
            "messages/sarama.go::SaramaGroup",
            "messages/sarama.go::SaramaPartition",
            "messages/sarama.go::SaramaProduce",
            "messages/sarama.go::orderHandler",
            "messages/sarama.go::orderHandler.Cleanup",
            "messages/sarama.go::orderHandler.ConsumeClaim",
            "messages/sarama.go::orderHandler.Setup",
            "messages/segmentio.go::ConsumeOrders",
            "messages/segmentio.go::ConsumeRefunds",
            "messages/segmentio.go::ConsumeUnknown",
            "messages/segmentio.go::EmitRefund",
            "messages/segmentio.go::ProduceAll",
            "messages/segmentio.go::ProduceMade",
            "messages/segmentio.go::ProduceMessages",
            "messages/segmentio.go::ProduceUnknown",
            "messages/segmentio.go::ReadLoop",
            "messages/segmentio.go::emit",
            "messages/segmentio.go::newWriter",
            "messages/shadowed.go::Outbox",
            "messages/shadowed.go::Outbox.Publish",
            "messages/shadowed.go::Outbox.WriteMessages",
            "messages/shadowed.go::Outbox.XAdd",
            "messages/shadowed.go::Shadowed",
            "messages/streams.go::RedisConsume",
            "messages/streams.go::RedisProduce",
            "messages/subjects.go::NatsPublish",
            "messages/subjects.go::NatsSubscribe",
            "messages/subjects.go::NatsSubscribe.nats orders.created",
            "messages/subjects.go::auditor",
            "messages/subjects.go::auditor.onAudit",
            "messages/subjects.go::onOrder",
            "main.go::main",
            "main.go::run",
            "methods/calls.go::run",
            "methods/calls.go::opaque",
            "methods/calls.go::UseBefore",
            "methods/calls.go::UseBeforeInClosure",
            "methods/calls.go::AfterBlock",
            "methods/calls.go::IfInitShadow",
            "methods/calls.go::SwitchInitShadow",
            "methods/calls.go::ForInitShadow",
            "methods/calls.go::TypeSwitchShadow",
            "methods/calls.go::RangeShadow",
            "methods/types.go::T",
            "methods/types.go::T.Ping",
            "methods/types.go::Box",
            "methods/types.go::Box.M",
            "methods/calls.go::FromParam",
            "methods/calls.go::FromValueParam",
            "methods/calls.go::FromVar",
            "methods/calls.go::FromLiteral",
            "methods/calls.go::FromVarLiteral",
            "methods/calls.go::FromResult",
            "methods/calls.go::FromSecondResult",
            "methods/calls.go::FromMethodResult",
            "methods/calls.go::FromQualifiedType",
            "methods/calls.go::FromSibling",
            "methods/calls.go::FromPackageVar",
            "methods/calls.go::FromInterface",
            "methods/calls.go::FromInterfaceAsShape",
            "methods/calls.go::FromClosure",
            "methods/calls.go::NotWritten",
            "methods/calls.go::NotWrittenMany",
            "methods/calls.go::Rebound",
            "methods/calls.go::ReboundByParam",
            "methods/calls.go::LocalType",
            "methods/calls.go::TypeParam",
            "methods/calls.go::Capture",
            "methods/types.go::Counter",
            "methods/types.go::Counter.Bump",
            "methods/types.go::Counter.Twice",
            "methods/types.go::Log",
            "methods/types.go::Log.Add",
            "methods/types.go::Gauge",
            "methods/types.go::Gauge.Add",
            "methods/types.go::Sink",
            "methods/types.go::Sink.Flush",
            "methods/types.go::Disk",
            "methods/types.go::Disk.Flush",
            "methods/types.go::Mem",
            "methods/types.go::Mem.Flush",
            "methods/types.go::Holder",
            "methods/types.go::Holder.Run",
            "methods/types.go::Holder.Counter",
            "methods/types.go::NewCounter",
            "methods/types.go::Open",
            "methods/types.go::Pair",
            "methods/reset.go::Counter.Reset",
            "report.go::report",
            "scopes/clash.go::Clash",
            "scopes/other.go::Other",
            "scopes/scopes.go::Box",
            "scopes/scopes.go::Box.Get",
            "scopes/scopes.go::Generic",
            "scopes/scopes.go::H",
            "scopes/scopes.go::H.Serve",
            "scopes/scopes.go::LocalConst",
            "scopes/scopes.go::LocalType",
            "scopes/scopes.go::Capture",
            "scopes/scopes.go::ForInit",
            "scopes/scopes.go::IfInit",
            "scopes/scopes.go::Import",
            "scopes/scopes.go::LiteralParam",
            "scopes/scopes.go::Named",
            "scopes/scopes.go::Param",
            "scopes/scopes.go::Plain",
            "scopes/scopes.go::Range",
            "scopes/scopes.go::Select",
            "scopes/scopes.go::ShortVar",
            "scopes/scopes.go::SwitchInit",
            "scopes/scopes.go::TypeSwitch",
            "scopes/scopes.go::Var",
            "scopes/scopes.go::outer",
            "scopes/scopes.go::pick",
            "scopes/scopes.go::run",
            "store/client_test.go::TestClient",
            "store/client_test.go::helper",
            "store/codec_pure.go::encode",
            "store/disk_linux.go::onLinux",
            "store/entry.go::main",
            "store/helpers.go::Exported",
            "store/helpers.go::helper",
            "store/store.go::New",
            "store/store.go::Store",
            "store/store.go::Store.Get",
            "store/store.go::init",
            "store/store.go::init#2",
            "store/store.go::open",
            "store/store_test.go::TestNew",
            "store/sys_unix.go::sysOpen",
            "tools/ext_test.go::main",
            "tools/runner.go::Runner",
            "tools/runner.go::Runner.main",
            "web/chi.go::ChiAdmin",
            "web/chi.go::ChiAny",
            "web/chi.go::ChiAudit",
            "web/chi.go::ChiEdit",
            "web/chi.go::ChiFiles",
            "web/chi.go::ChiGuard",
            "web/chi.go::ChiHidden",
            "web/chi.go::ChiHiddenNames",
            "web/chi.go::ChiIndex",
            "web/chi.go::ChiMake",
            "web/chi.go::ChiNest",
            "web/chi.go::ChiPassed",
            "web/chi.go::ChiPing",
            "web/chi.go::ChiRoutes",
            "web/chi.go::ChiRoutes.DELETE /things/{}",
            "web/chi.go::ChiRebound",
            "web/chi.go::ChiShadow",
            "web/chi.go::ChiShow",
            "web/chi.go::ChiTeam",
            "web/chi.go::ChiTwice",
            "web/chi.go::ChiUnread",
            "web/chi.go::adminRoutes",
            "web/chi.go::chiMany",
            "web/chi.go::chiOther",
            "web/chi.go::deepRoutes",
            "web/chi.go::passedLoop",
            "web/chi.go::passedNever",
            "web/chi.go::passedStats",
            "web/chi.go::passedUsers",
            "web/chi.go::teamRoutes",
            "web/chi.go::unreadable",
            "web/echo.go::Auth",
            "web/echo.go::Billing",
            "web/echo.go::Create",
            "web/echo.go::EchoDynamic",
            "web/echo.go::EchoRoutes",
            "web/echo.go::EchoRoutes.DELETE /api/items/{}",
            "web/echo.go::EchoWrapped",
            "web/echo.go::Item",
            "web/echo.go::RegisterBilling",
            "web/echo.go::Wide",
            "web/echo.go::Wide2",
            "web/gin.go::Audit",
            "web/gin.go::GinCaptured",
            "web/gin.go::GinEscaped",
            "web/gin.go::GinHidden",
            "web/gin.go::GinReassigned",
            "web/gin.go::GinRoutes",
            "web/gin.go::GinRoutes.DELETE /v1/users/{}",
            "web/gin.go::GinShadow",
            "web/gin.go::GinWrapped",
            "web/gin.go::Hidden",
            "web/gin.go::Hidden2",
            "web/gin.go::Mid",
            "web/gin.go::Operations",
            "web/gin.go::RegisterAudit",
            "web/gin.go::Save",
            "web/gin.go::Serve",
            "web/gin.go::Show",
            "web/gin.go::Status",
            "web/gin.go::other",
            "web/gin.go::setPath",
            "web/gorilla.go::GorillaHidden",
            "web/gorilla.go::GorillaRoutes",
            "web/gorilla.go::GorillaRoutes.DELETE /inline",
            "web/gorilla.go::GorillaShadow",
            "web/gorilla.go::GorillaUnread",
            "web/gorilla.go::MuxAny",
            "web/gorilla.go::MuxHidden",
            "web/gorilla.go::MuxIndex",
            "web/gorilla.go::MuxMake",
            "web/gorilla.go::MuxOrder",
            "web/gorilla.go::MuxShow",
            "web/gorilla.go::RegisterOrders",
            "web/gorilla.go::muxOther",
            "web/stdlib.go::CreateUser",
            "web/stdlib.go::Default",
            "web/stdlib.go::Echo",
            "web/stdlib.go::Files",
            "web/stdlib.go::GetUser",
            "web/stdlib.go::Guard",
            "web/stdlib.go::Health",
            "web/stdlib.go::Home",
            "web/stdlib.go::Hosted",
            "web/stdlib.go::Ledger",
            "web/stdlib.go::Literals",
            "web/stdlib.go::Mounted",
            "web/stdlib.go::Mux",
            "web/stdlib.go::Mux.DELETE /users/{}",
            "web/stdlib.go::Ping",
            "web/stdlib.go::Register",
            "web/stdlib.go::Server",
            "web/stdlib.go::Server.Routes",
            "web/stdlib.go::Stats",
            "web/stdlib.go::Wrapped",
            "web/stdlib.go::newAPI",
            "web/stdlib.go::newGuarded",
            "web/stdlib.go::newStripped",
            "worker/jobs/jobs.go::Run",
            "worker/main.go::main",
        ],
        &["main.go::main", "worker/main.go::main"],
        &[
            // A function of the same file, and one called inside a function literal, which
            // belongs to the function around it.
            (
                "internal/shapes/shapes.go::NewCircle::clamp#0",
                EXACT,
                &["internal/shapes/shapes.go::clamp"],
            ),
            (
                "internal/shapes/shapes.go::Each::clamp#0",
                EXACT,
                &["internal/shapes/shapes.go::clamp"],
            ),
            // A parameter called is no definition.
            ("internal/shapes/shapes.go::Each::visit#0", None, &[]),
            // A plain import names the directory the module path gives; an aliased one too.
            (
                "main.go::main::shapes.NewCircle#0",
                EXACT,
                &["internal/shapes/shapes.go::NewCircle"],
            ),
            (
                "main.go::run::shapes.Each#0",
                EXACT,
                &["internal/shapes/shapes.go::Each"],
            ),
            (
                "report.go::report::sh.NewCircle#0",
                EXACT,
                &["internal/shapes/shapes.go::NewCircle"],
            ),
            // A dot import: the package's exports join the file scope.
            (
                "main.go::main::Meters#0",
                EXACT,
                &["internal/units/units.go::Meters"],
            ),
            ("main.go::main::run#0", EXACT, &["main.go::run"]),
            // A method on the value a constructor returns.
            (
                "main.go::main::c.Area#0",
                EXACT,
                &["internal/shapes/shapes.go::Circle.Area"],
            ),
            // The standard library and modules outside the workspace are external.
            ("main.go::main::fmt.Println#0", None, &[]),
            ("main.go::main::str.ToUpper#0", None, &[]),
            ("main.go::main::gin.Default#0", None, &[]),
            // A function of another file of the package.
            ("report.go::report::run#0", EXACT, &["main.go::run"]),
            (
                "store/store.go::New::helper#0",
                EXACT,
                &["store/helpers.go::helper"],
            ),
            // Each `init` is a symbol of its own; a blank function is none.
            (
                "store/store.go::init::open#0",
                EXACT,
                &["store/store.go::open"],
            ),
            (
                "store/store.go::init#2::helper#0",
                EXACT,
                &["store/helpers.go::helper"],
            ),
            ("store/store.go::open#0", EXACT, &["store/store.go::open"]),
            (
                "store/helpers.go::Exported::open#0",
                EXACT,
                &["store/store.go::open"],
            ),
            // A test file of the package shares its scope; the external test package does not.
            (
                "store/store_test.go::TestNew::New#0",
                EXACT,
                &["store/store.go::New"],
            ),
            (
                "store/store_test.go::TestNew::helper#0",
                EXACT,
                &["store/helpers.go::helper"],
            ),
            (
                "store/client_test.go::TestClient::helper#0",
                EXACT,
                &["store/client_test.go::helper"],
            ),
            (
                "store/client_test.go::TestClient::store.New#0",
                EXACT,
                &["store/store.go::New"],
            ),
            // An import names every file of the directory.
            ("main.go::run::store.New#0", EXACT, &["store/store.go::New"]),
            (
                "main.go::run::store.Exported#0",
                EXACT,
                &["store/helpers.go::Exported"],
            ),
            ("main.go::run::store.helper#0", None, &[]),
            (
                "store/disk_linux.go::onLinux::helper#0",
                EXACT,
                &["store/helpers.go::helper"],
            ),
            // A `replace` to a local directory; a nested module owns its subtree.
            (
                "report.go::report::invoice.Total#0",
                EXACT,
                &["forks/billing/invoice/invoice.go::Total"],
            ),
            ("report.go::report::jobs.Run#0", None, &[]),
            (
                "worker/main.go::main::jobs.Run#0",
                EXACT,
                &["worker/jobs/jobs.go::Run"],
            ),
            ("worker/main.go::main::invoice.Total#0", None, &[]),
            // Hidden names: a call through the inner binding is a name match at most.
            ("scopes/scopes.go::Plain::outer#0", EXACT, &[OUTER]),
            (
                "scopes/scopes.go::Plain::store.New#0",
                EXACT,
                &["store/store.go::New"],
            ),
            ("scopes/other.go::Other::outer#0", EXACT, &[OUTER]),
            ("scopes/scopes.go::ShortVar::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Var::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::IfInit::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::SwitchInit::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::ForInit::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Range::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::TypeSwitch::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Select::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Param::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Named::outer#0", INFERRED, &[OUTER]),
            (
                "scopes/scopes.go::LiteralParam::outer#0",
                INFERRED,
                &[OUTER],
            ),
            ("scopes/scopes.go::Capture::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Import::store.New#0", None, &[]),
            // Type parameters, a receiver's type arguments, the receiver, local types and
            // constants, and the parameters of a blank function and of a top-level literal.
            ("scopes/scopes.go::Generic::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::Box.Get::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::H.Serve::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::LocalType::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::LocalConst::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::outer#0", INFERRED, &[OUTER]),
            ("scopes/scopes.go::outer#1", INFERRED, &[OUTER]),
            // Two imports that give one name leave it unknown.
            ("scopes/clash.go::Clash::http.NewRequest#0", None, &[]),
            // A build constraint after a block comment counts.
            (
                "store/store.go::New::sysOpen#0",
                EXACT,
                &["store/sys_unix.go::sysOpen"],
            ),
            // A cgo file is left out, as the target builds without cgo.
            (
                "store/store.go::New::encode#0",
                EXACT,
                &["store/codec_pure.go::encode"],
            ),
            // A replace by a module from outside makes the module of the repository outside.
            ("report.go::report::old.Old#0", None, &[]),
            (
                "worker/main.go::main::old.Old#0",
                EXACT,
                &["legacy/old/old.go::Old"],
            ),
            // Method calls (spec #25): a receiver whose type is written is exact, one whose type
            // is not is a name match, and an interface stops at its own method.
            (
                "methods/calls.go::FromParam::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromValueParam::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromVar::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromLiteral::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromVarLiteral::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromResult::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromResult::d.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromResult::e.Area#0",
                EXACT,
                &["internal/shapes/shapes.go::Circle.Area"],
            ),
            (
                "methods/calls.go::FromSecondResult::l.Add#0",
                EXACT,
                &["methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::FromSecondResult::g.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::FromMethodResult::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromQualifiedType::c.Area#0",
                EXACT,
                &["internal/shapes/shapes.go::Circle.Area"],
            ),
            (
                "methods/calls.go::FromSibling::c.Reset#0",
                EXACT,
                &["methods/reset.go::Counter.Reset"],
            ),
            (
                "methods/calls.go::FromPackageVar::shared.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromPackageVar::global.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromInterface::s.Flush#0",
                EXACT,
                &["methods/types.go::Sink.Flush"],
            ),
            (
                "methods/calls.go::FromInterfaceAsShape::s.Area#0",
                EXACT,
                &["internal/shapes/shapes.go::Shape.Area"],
            ),
            (
                "methods/calls.go::FromClosure::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::FromClosure::c.Add#0",
                EXACT,
                &["methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::NotWritten::c.Bump#0",
                INFERRED,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::NotWrittenMany::l.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::Rebound::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::Rebound::c.Bump#0",
                INFERRED,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::ReboundByParam::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::ReboundByParam::c.Bump#0",
                INFERRED,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::LocalType::c.Bump#0",
                INFERRED,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/calls.go::TypeParam::s.Flush#0",
                AMBIGUOUS,
                &[
                    "methods/types.go::Disk.Flush",
                    "methods/types.go::Mem.Flush",
                    "methods/types.go::Sink.Flush",
                ],
            ),
            (
                "methods/calls.go::Capture::c.Add#0",
                EXACT,
                &["methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::Capture::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/types.go::Counter.Twice::c.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/types.go::Holder.Run::h.counter.Bump#0",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            (
                "methods/types.go::Holder.Run::h.log.Add#0",
                EXACT,
                &["methods/types.go::Log.Add"],
            ),
            (
                "methods/types.go::Holder.Run::h.sink.Flush#0",
                EXACT,
                &["methods/types.go::Sink.Flush"],
            ),
            (
                "methods/types.go::Holder.Run::h.circle.Area#0",
                EXACT,
                &["internal/shapes/shapes.go::Circle.Area"],
            ),
            (
                "methods/types.go::Counter.Twice::c.Bump#1",
                EXACT,
                &["methods/types.go::Counter.Bump"],
            ),
            // A call before the binding that hides a name, a block, and every binding form.
            (
                "methods/calls.go::UseBefore::cur.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::UseBeforeInClosure::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::AfterBlock::cur.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::AfterBlock::cur.Add#1",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::IfInitShadow::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::SwitchInitShadow::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::ForInitShadow::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::TypeSwitchShadow::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            (
                "methods/calls.go::RangeShadow::c.Add#0",
                AMBIGUOUS,
                &["methods/types.go::Gauge.Add", "methods/types.go::Log.Add"],
            ),
            // A type declaration's parameters scope the declaration, not its methods.
            (
                "methods/types.go::Box.M::t.Ping#0",
                EXACT,
                &["methods/types.go::T.Ping"],
            ),
            (
                "methods/types.go::Box.M::b.v.Ping#0",
                INFERRED,
                &["methods/types.go::T.Ping"],
            ),
        ],
    );
    // Files the build constraints leave out for `linux/amd64` are not indexed.
    let repo = fixture_repo("conformance_go_absent", "conformance-go", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(!export.contains("disk_windows.go"));
    assert!(!export.contains("legacy.go"));
    assert!(!export.contains("sys_other.go"));
}

/// A chi subrouter passed on through exact calls is followed to a limit: a chain of ten
/// functions is served under the prefix of its `Route`, one past the limit is served nowhere.
#[test]
fn a_chi_subrouter_passed_on_past_the_limit_is_not_read() {
    let repo = fixture_repo("go_chi_chain", "conformance-go", Some(ORIGIN));
    let chain = |name: &str, links: usize| {
        let calls: String = (0..links)
            .map(|i| format!("func {name}{i}(r chi.Router) {{ {name}{}(r) }}\n", i + 1))
            .collect();
        format!(
            "{calls}func {name}{links}(r chi.Router) {{ r.Get(\"/end\", {name}Last) }}\nfunc {name}Last() {{}}\n"
        )
    };
    let source = format!(
        "package web\n\nimport \"github.com/go-chi/chi/v5\"\n\n\
         func Chains() {{\n\tr := chi.NewRouter()\n\
         \tr.Route(\"/short\", short0)\n\tr.Route(\"/long\", long0)\n}}\n{}{}",
        chain("short", 10),
        chain("long", 70)
    );
    std::fs::write(repo.join("web/chain.go"), source).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(export.contains("http::GET::/short/end"));
    assert!(!export.contains("http::GET::/long/end"));
}

/// The routers of the Go fixture (spec #25): `net/http` patterns, and gin, echo, chi and
/// gorilla/mux groups, with every prefix a router is registered under (chi's `Route`, `Group`
/// and `Mount`, gorilla's `PathPrefix(..).Subrouter()`), a group passed to or returned from a
/// function, a function literal as the handler and a router reached under two prefixes. A name
/// a block, a loop, a literal or an assignment binds again, an import a local hides, a router
/// mounted inside a call and a prefix or a method Meada cannot read leave no provider.
#[test]
fn go_routes_are_providers_under_every_prefix() {
    let (_, actual) = providers("go_routes", "conformance-go", "web/");
    let mut expected = pairs(&[
        // The default mux and a conversion to `HandlerFunc`.
        ("http::ANY::/health", "web/stdlib.go::Health"),
        ("http::ANY::/echo", "web/stdlib.go::Echo"),
        // Go 1.22 patterns: a method, parameters, a rest wildcard, a dropped host, the anchor.
        ("http::GET::/users/{}", "web/stdlib.go::GetUser"),
        ("http::POST::/users", "web/stdlib.go::CreateUser"),
        ("http::ANY::/files/{}", "web/stdlib.go::Files"),
        ("http::ANY::/hosted", "web/stdlib.go::Hosted"),
        ("http::GET::/", "web/stdlib.go::Home"),
        (
            "http::DELETE::/users/{}",
            "web/stdlib.go::Mux.DELETE /users/{}",
        ),
        ("http::GET::/ping", "web/stdlib.go::Ping"),
        // A mux given as the handler sees the whole path: its routes are served where they fall
        // under the outer pattern and for the outer method alone (`POST /a2` is outside `/sub/`,
        // `POST /static/b` is not `GET`, `/exact/deeper` is below an exact pattern).
        ("http::GET::/plain/ledger", "web/stdlib.go::Ledger"),
        ("http::GET::/sub/a1", "web/stdlib.go::Ledger"),
        ("http::ANY::/sub/any", "web/stdlib.go::Ledger"),
        ("http::GET::/static/a", "web/stdlib.go::Ledger"),
        ("http::ANY::/exact", "web/stdlib.go::Ledger"),
        // A mux a function returns is followed to where it is mounted. One inside
        // `StripPrefix`, a middleware, `echo.WrapHandler` or `gin.WrapH`, or mounted on a router
        // held in a field, is an unread mount: served nowhere (and no route for the call).
        ("http::GET::/returned/k1", "web/stdlib.go::Ledger"),
        // `net/http` has no `:name`; its `*` is a literal.
        ("http::ANY::/star/*", "web/stdlib.go::Stats"),
        // The route of the wrapper call is still read; its handler is not a function.
        ("http::ANY::/legacy/{}", "web/echo.go"),
        ("http::ANY::/legacy/{}", "web/gin.go"),
        // gin: nested groups, every method, `Handle`, `Any`, the last of several handlers and a
        // group made in a chain.
        ("http::GET::/v1/users/{}", "web/gin.go::Show"),
        ("http::POST::/v1/users", "web/gin.go::Save"),
        ("http::PUT::/v1/users/{}", "web/gin.go::Save"),
        ("http::PATCH::/v1/users/{}", "web/gin.go::Save"),
        ("http::HEAD::/v1/users/{}", "web/gin.go::Show"),
        ("http::OPTIONS::/v1/users/{}", "web/gin.go::Show"),
        (
            "http::DELETE::/v1/users/{}",
            "web/gin.go::GinRoutes.DELETE /v1/users/{}",
        ),
        ("http::GET::/v1/users/{}/tags", "web/gin.go::Show"),
        ("http::POST::/v1/users/{}/tags", "web/gin.go::Save"),
        ("http::ANY::/v1/users/any", "web/gin.go::Save"),
        ("http::GET::/files/{}", "web/gin.go::Serve"),
        ("http::GET::/mw/chained", "web/gin.go::Show"),
        // A group passed to a function: one provider per prefix, and the cycle adds none.
        ("http::GET::/v1/audit", "web/gin.go::Audit"),
        ("http::GET::/admin/audit", "web/gin.go::Audit"),
        // A group a function returns, and a router a closure captures.
        ("http::GET::/ops/status", "web/gin.go::Status"),
        ("http::GET::/captured", "web/gin.go::Show"),
        // echo: the handler is the second argument, middleware follows.
        ("http::GET::/api/items/{}", "web/echo.go::Item"),
        ("http::POST::/api/items", "web/echo.go::Create"),
        ("http::PUT::/api/items/{}", "web/echo.go::Create"),
        ("http::PATCH::/api/items/{}", "web/echo.go::Create"),
        (
            "http::DELETE::/api/items/{}",
            "web/echo.go::EchoRoutes.DELETE /api/items/{}",
        ),
        ("http::HEAD::/api/items/{}", "web/echo.go::Item"),
        ("http::OPTIONS::/api/items/{}", "web/echo.go::Item"),
        ("http::ANY::/api/items/any", "web/echo.go::Wide"),
        ("http::GET::/static/{}", "web/echo.go::Wide"),
        ("http::GET::/api/billing", "web/echo.go::Billing"),
        ("http::GET::/internal/billing", "web/echo.go::Billing"),
        // chi: every method, `Method`, `HandleFunc` and `Handle` for every method, a wildcard.
        ("http::GET::/ping", "web/chi.go::ChiPing"),
        ("http::POST::/things", "web/chi.go::ChiMake"),
        ("http::PUT::/things/{}", "web/chi.go::ChiEdit"),
        ("http::PATCH::/things/{}/patch", "web/chi.go::ChiEdit"),
        (
            "http::DELETE::/things/{}",
            "web/chi.go::ChiRoutes.DELETE /things/{}",
        ),
        ("http::HEAD::/probe", "web/chi.go::ChiPing"),
        ("http::OPTIONS::/probe", "web/chi.go::ChiPing"),
        ("http::CONNECT::/probe", "web/chi.go::ChiPing"),
        ("http::TRACE::/probe", "web/chi.go::ChiPing"),
        ("http::GET::/method", "web/chi.go::ChiShow"),
        ("http::POST::/method", "web/chi.go::ChiMake"),
        ("http::GET::/lower", "web/chi.go::ChiShow"),
        ("http::ANY::/any", "web/chi.go::ChiAny"),
        ("http::ANY::/handled", "web/chi.go::ChiAny"),
        ("http::GET::/files/{}", "web/chi.go::ChiFiles"),
        // `Route` with a literal, nested and a named function; `Group` adds no prefix.
        ("http::GET::/rooms", "web/chi.go::ChiIndex"),
        ("http::GET::/rooms/members/{}", "web/chi.go::ChiShow"),
        ("http::PUT::/rooms/members/{}", "web/chi.go::ChiEdit"),
        ("http::GET::/rooms/teams", "web/chi.go::ChiTeam"),
        ("http::GET::/rooms/teams/{}/members", "web/chi.go::ChiTeam"),
        ("http::GET::/grouped", "web/chi.go::ChiPing"),
        // `Mount` of a router a function returns, mounted again inside it, and one router under
        // two prefixes.
        ("http::GET::/admin/stats", "web/chi.go::ChiAdmin"),
        ("http::GET::/admin/deep/x", "web/chi.go::ChiAdmin"),
        ("http::GET::/a/shared", "web/chi.go::ChiAdmin"),
        ("http::GET::/b/shared", "web/chi.go::ChiAdmin"),
        // The subrouter of a `Route` function passed on to a function, through a name and to
        // itself, and the same function called with another router.
        ("http::GET::/passed/users", "web/chi.go::ChiTeam"),
        ("http::GET::/users", "web/chi.go::ChiTeam"),
        ("http::GET::/passed/stats", "web/chi.go::ChiAdmin"),
        ("http::GET::/passed/loop", "web/chi.go::ChiPing"),
        // A router nested under itself: the cycle adds nothing.
        ("http::GET::/audit", "web/chi.go::ChiTeam"),
        ("http::GET::/nested/audit", "web/chi.go::ChiTeam"),
        // gorilla/mux: every method without `.Methods(..)`, one provider for each method it
        // names, a regular expression in a parameter, and `Name` between the two calls.
        ("http::ANY::/", "web/gorilla.go::MuxIndex"),
        ("http::GET::/items/{}", "web/gorilla.go::MuxShow"),
        ("http::GET::/items/{}/tags", "web/gorilla.go::MuxShow"),
        ("http::HEAD::/items/{}/tags", "web/gorilla.go::MuxShow"),
        ("http::POST::/items", "web/gorilla.go::MuxMake"),
        ("http::PUT::/named", "web/gorilla.go::MuxShow"),
        ("http::POST::/lower", "web/gorilla.go::MuxMake"),
        (
            "http::DELETE::/inline",
            "web/gorilla.go::GorillaRoutes.DELETE /inline",
        ),
        ("http::ANY::/handled", "web/gorilla.go::MuxAny"),
        // Subrouters of `PathPrefix`, nested, chained, and passed to a function under two
        // prefixes (the cycle adds nothing).
        ("http::GET::/api/users", "web/gorilla.go::MuxShow"),
        ("http::GET::/api/v2/users/{}", "web/gorilla.go::MuxShow"),
        ("http::GET::/chain/c", "web/gorilla.go::MuxShow"),
        ("http::POST::/api/orders", "web/gorilla.go::MuxOrder"),
        ("http::POST::/shop/orders", "web/gorilla.go::MuxOrder"),
    ]);
    expected.sort();
    assert_eq!(actual, expected);
}

/// The consumers of the Go fixture's `client/` (spec #25): `net/http` calls and requests, resty
/// requests with the base of their client, URLs folded from literals, `+`, `fmt.Sprintf`,
/// constants and package-level names (of the package, of another file and of another package), and
/// wrappers read at their call sites. A path that cannot be evaluated is an unresolved consumer.
#[test]
fn go_http_calls_are_consumers_with_their_urls_folded() {
    let repo = fixture_repo("go_consumers", "conformance-go", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let api = "host:api.example.com";
    let net = |function: &str, call: &str| format!("client/net.go::{function}::{call}");
    let wrap = |function: &str, call: &str| format!("client/wrappers.go::{function}::{call}");
    let rest = |function: &str, call: &str| format!("client/rest.go::{function}::{call}");
    let field = |function: &str, call: &str| format!("client/fields.go::{function}::{call}");
    let chain = |verb: &str, n: usize| format!("(call_expression).{verb}#{n}");
    let expected = triples(&[
        // The package, a `Client`, the default client and a client written as a literal.
        ("http::GET::/health", api, &net("Package", "http.Get#0")),
        ("http::HEAD::/ping", api, &net("Package", "http.Head#0")),
        ("http::POST::/items", api, &net("Package", "http.Post#0")),
        (
            "http::POST::/forms",
            api,
            &net("Package", "http.PostForm#0"),
        ),
        ("http::GET::/on-client", api, &net("Clients", "c.Get#0")),
        ("http::HEAD::/on-client", api, &net("Clients", "c.Head#0")),
        ("http::POST::/on-client", api, &net("Clients", "c.Post#0")),
        (
            "http::POST::/on-client",
            api,
            &net("Clients", "c.PostForm#0"),
        ),
        (
            "http::GET::/on-default",
            api,
            &net("Clients", "http.DefaultClient.Get#0"),
        ),
        ("http::GET::/on-local", api, &net("Clients", "local.Get#0")),
        // The construction of a request is the consumer; a method that is no literal is open.
        (
            "http::GET::/req/get",
            api,
            &net("Requests", "http.NewRequest#0"),
        ),
        (
            "http::POST::/req/post",
            api,
            &net("Requests", "http.NewRequest#1"),
        ),
        (
            "http::DELETE::/req/delete",
            api,
            &net("Requests", "http.NewRequestWithContext#0"),
        ),
        (
            "unresolved",
            api,
            &net("Requests", "http.NewRequestWithContext#1"),
        ),
        // Literals, `+`, `fmt.Sprintf`, constants and names of the package and of others.
        ("http::GET::/raw", api, &net("Urls", "http.Get#0")),
        ("http::GET::/users/{}", api, &net("Urls", "http.Get#1")),
        (
            "http::GET::/users/{}/orders",
            api,
            &net("Urls", "http.Get#2"),
        ),
        ("http::GET::/search/{}", api, &net("Urls", "http.Get#3")),
        ("http::GET::/orders", api, &net("Urls", "http.Get#4")),
        ("http::GET::/users/{}", api, &net("Urls", "http.Get#5")),
        ("http::GET::/v2/teams", api, &net("Urls", "http.Get#6")),
        // A variable assigned in a function, in another file or in another package holds no value.
        ("unresolved", "", &net("Urls", "http.Get#7")),
        ("unresolved", "", &net("Urls", "http.Get#8")),
        ("unresolved", "", &net("Urls", "http.Get#9")),
        ("unresolved", "", &net("Unresolved", "http.Get#0")),
        ("unresolved", "", &net("Unresolved", "http.Get#1")),
        ("unresolved", "", &net("Unresolved", "http.Get#2")),
        // Wrappers: one level, two levels, a method parameter, an unreadable argument, a cycle.
        ("http::GET::/wrapped", api, &wrap("Wrapped", "get#0")),
        ("http::GET::/users/42", api, &wrap("Wrapped", "getUser#0")),
        ("http::PUT::/wrapped/put", api, &wrap("Wrapped", "send#0")),
        ("unresolved", "", &wrap("Wrapped", "get#1")),
        ("http::GET::/cycle", api, &wrap("Cycle", "ping#0")),
        ("http::GET::/", api, &wrap("pong", "ping#0")),
        // resty: the base URL of the client that issued the request.
        (
            "http::GET::/invoices",
            "host:billing.example.com",
            &rest("Rest", &chain("Get", 0)),
        ),
        (
            "http::POST::/invoices",
            "host:billing.example.com",
            &rest("Rest", &chain("Post", 0)),
        ),
        (
            "http::PUT::/invoices/1",
            "host:billing.example.com",
            &rest("Rest", &chain("Put", 0)),
        ),
        (
            "http::PATCH::/invoices/1",
            "host:billing.example.com",
            &rest("Rest", &chain("Patch", 0)),
        ),
        (
            "http::DELETE::/invoices/1",
            "host:billing.example.com",
            &rest("Rest", &chain("Delete", 0)),
        ),
        (
            "http::HEAD::/invoices",
            "host:billing.example.com",
            &rest("Rest", &chain("Head", 0)),
        ),
        (
            "http::OPTIONS::/invoices",
            "host:billing.example.com",
            &rest("Rest", &chain("Options", 0)),
        ),
        (
            "http::GET::/executed",
            "host:billing.example.com",
            &rest("Rest", &chain("Execute", 0)),
        ),
        (
            "http::POST::/executed",
            "host:billing.example.com",
            &rest("Rest", &chain("Execute", 1)),
        ),
        (
            "http::GET::/stored",
            "host:billing.example.com",
            &rest("Rest", "req.Get#0"),
        ),
        (
            "http::GET::/absolute",
            "host:other.example.com",
            &rest("Rest", &chain("Get", 1)),
        ),
        (
            "http::GET::/entries",
            "host:ledger.example.com",
            &rest("Separate", &chain("Get", 0)),
        ),
        (
            "http::GET::/plain",
            "host:bare.example.com",
            &rest("Separate", &chain("Get", 1)),
        ),
        // A client written as a type has no base; a client of two bases has an unknown one.
        ("http::GET::/typed", "", &rest("Typed", &chain("Get", 0))),
        ("http::GET::/typed-request", "", &rest("Typed", "r.Get#0")),
        (
            "http::GET::/items",
            "",
            &rest("Gateway.Items", &chain("Get", 0)),
        ),
        (
            "http::GET::/plain",
            api,
            &rest("Gateway.Items", "g.plain.Get#0"),
        ),
        (
            "http::GET::/returned",
            "host:returned.example.com",
            &rest("Returned", &chain("Get", 0)),
        ),
        ("http::GET::/reused", "", &rest("Reused", &chain("Get", 0))),
        (
            "http::GET::/wrapped-rest",
            "host:billing.example.com",
            &rest("Wrapper", "fetchJSON#0"),
        ),
        // Two clients one factory makes keep their own bases; a wrapper given a traced client and
        // one that is not has an unknown base; a client assigned again is no consumer.
        (
            "http::GET::/from-a",
            "host:a.example.com",
            &rest("Factory", &chain("Get", 1)),
        ),
        ("http::GET::/from-b", "", &rest("Factory", &chain("Get", 0))),
        ("http::GET::/m1", "", &rest("Mixed", "fetchBase#0")),
        ("http::GET::/m2", "", &rest("Mixed", "fetchBase#1")),
        // A base URL read from the environment, by `os.Getenv` and by `os.LookupEnv` with a
        // fallback.
        (
            "http::GET::/env/get",
            "env:API_URL",
            "client/env.go::FromEnv::http.Get#0",
        ),
        (
            "http::GET::/env/lookup",
            "env:BILLING_URL",
            &format!("client/env.go::FromLookup::{}", chain("Get", 0)),
        ),
        // A client kept in a struct field: the literals that set the field give its base. Two
        // bases, a literal whose client is not traced, a selector write of the field, a base set
        // through the field and a literal without field names leave it unknown; a field no
        // literal sets is the client its type names.
        (
            "http::GET::/v1/items",
            "host:api.example.com",
            &field("Billing.Items", &chain("Get", 0)),
        ),
        (
            "http::GET::/entries",
            "host:ledger.example.com",
            &field("Ledger.Entries", &chain("Get", 0)),
        ),
        (
            "http::GET::/sync",
            "",
            &field("Mirror.Sync", &chain("Get", 0)),
        ),
        (
            "http::GET::/forward",
            "",
            &field("Proxy.Forward", &chain("Get", 0)),
        ),
        (
            "http::POST::/send",
            "",
            &field("Relay.Send", &chain("Post", 0)),
        ),
        (
            "http::POST::/push",
            "",
            &field("Queue.Push", &chain("Post", 0)),
        ),
        (
            "http::GET::/left",
            "",
            &field("Pair.Left", &chain("Get", 0)),
        ),
        (
            "http::GET::/ping",
            "",
            &field("Pool.Lease", &chain("Get", 0)),
        ),
        (
            "http::GET::/poll",
            "",
            &field("Unset.Idle", &chain("Get", 0)),
        ),
        // A package variable another package writes through a selector (`config.Base = ..`,
        // `&config.Path`, `config.Mode += ..`) is no constant; `Host` of a request is no write
        // to the constant `Host`.
        ("http::GET::/a", "", &net("Written", "http.Get#0")),
        ("unresolved", "", &net("Written", "http.Get#1")),
        ("unresolved", "", &net("Written", "http.Get#2")),
        ("http::GET::/stable", api, &net("Written", "http.Get#3")),
        // A flag, width or precision makes a hole; `%d` prints integers only.
        ("http::GET::/pad/{}", api, &net("Formats", "http.Get#0")),
        ("http::GET::/prec/{}", api, &net("Formats", "http.Get#1")),
        ("http::GET::/num/{}", api, &net("Formats", "http.Get#2")),
        ("http::GET::/v2/items/7", api, &net("Formats", "http.Get#3")),
        (
            "http::GET::/plain/ab/{}",
            api,
            &net("Formats", "http.Get#4"),
        ),
    ]);
    assert_eq!(consumers(&json, "client/"), expected);
}

/// The axum service of the Rust fixture (spec #30): routes with every `nest` prefix, a router
/// built in another module, one router under two prefixes, chained method routers, `any`, a
/// closure handler and the wildcard spellings. A router mounted under a prefix Meada cannot
/// read, or on a router held in a field, is served nowhere, and an Express-spelled `Router` of
/// the crate's own is no provider.
#[test]
fn rust_axum_routes_are_providers_under_every_prefix() {
    let (_, actual) = providers("rust_axum", "conformance-rust", "crates/web/src/");
    let items = |prefix: &str| {
        let at = |path: &str, m: &str, handler: &str| {
            (
                format!("http::{m}::{prefix}{path}"),
                format!("crates/web/src/items.rs::{handler}"),
            )
        };
        vec![
            at("/items", "GET", "list"),
            at("/items", "POST", "create"),
            at("/items/{}", "GET", "show"),
            at("/items/{}", "PUT", "update"),
            at("/items/{}", "PATCH", "update"),
            at("/items/{}", "DELETE", "remove"),
            at("/items/{}/tags", "POST", "create"),
            at("/items/{}/tags", "HEAD", "show"),
            at("/items/{}/tags", "OPTIONS", "show"),
        ]
    };
    let mut expected: Vec<(String, String)> = [
        // `nest(API, api)` reads the constant from another module; the closure is named for
        // its route.
        (
            "http::GET::/api/ping",
            "crates/web/src/lib.rs::app.GET /api/ping",
        ),
        // `merge` adds no prefix; `any` serves every method.
        ("http::GET::/health", "crates/web/src/health.rs::check"),
        ("http::ANY::/echo", "crates/web/src/health.rs::echo"),
        ("http::GET::/files/{}", "crates/web/src/files.rs::read"),
        ("http::GET::/blobs/{}", "crates/web/src/files.rs::read"),
        // A router a function receives and returns, mounted under a literal.
        ("http::GET::/admin/stats", "crates/web/src/admin.rs::stats"),
        ("http::GET::/legacy/old", "crates/web/src/health.rs::check"),
        // `{{raw}}` is the literal `{raw}`, not a parameter.
        ("http::GET::/{raw}", "crates/web/src/health.rs::check"),
        // The method is axum's name for the function, not the file's.
        (
            "http::GET::/alias/renamed",
            "crates/web/src/health.rs::check",
        ),
        (
            "http::ANY::/alias/renamed-any",
            "crates/web/src/health.rs::echo",
        ),
        ("http::POST::/admin/reset", "crates/web/src/admin.rs::reset"),
        // `orphans::router` is nested under a prefix that is a parameter: no provider at
        // `/never`, under any prefix.
    ]
    .into_iter()
    .map(|(c, s)| (c.to_string(), s.to_string()))
    .collect();
    expected.extend(items("/api/v1"));
    expected.extend(items("/api/v2"));
    expected.sort();
    assert_eq!(actual, expected);
}

/// The actix-web service of the Rust fixture (spec #30): attribute routes (`route` with several
/// methods included) registered bare, in a tuple, in nested scopes and through a function that
/// returns a scope; route builders on the app, resources and the service configurations
/// `.configure` takes. A prefix read at run time serves nothing, a handler no call names is
/// served at its own path, one a call names on a receiver Meada cannot read is served nowhere,
/// and another crate's `get` attribute is no route.
#[test]
fn rust_actix_routes_are_providers_under_every_mount() {
    let (_, actual) = providers("rust_actix", "conformance-rust", "crates/actix/src/");
    let expected = pairs(&[
        // Attributes registered bare and in a tuple.
        ("http::GET::/accounts", "crates/actix/src/accounts.rs::list"),
        (
            "http::POST::/accounts",
            "crates/actix/src/accounts.rs::create",
        ),
        (
            "http::DELETE::/accounts/{}",
            "crates/actix/src/accounts.rs::remove",
        ),
        // A scope nested in a scope; the handler is not served at the root as well.
        (
            "http::GET::/v1/admin/accounts/{}",
            "crates/actix/src/accounts.rs::show",
        ),
        // A function that returns a scope, `#[route]` with two methods, `{tail:.*}`.
        (
            "http::PUT::/api/accounts/{}/sync",
            "crates/actix/src/accounts.rs::sync",
        ),
        (
            "http::PATCH::/api/accounts/{}/sync",
            "crates/actix/src/accounts.rs::sync",
        ),
        (
            "http::GET::/api/files/{}",
            "crates/actix/src/accounts.rs::files",
        ),
        // Route builders on the app.
        ("http::GET::/ping", "crates/actix/src/orders.rs::ping"),
        ("http::ANY::/echo", "crates/actix/src/orders.rs::echo"),
        (
            "http::PUT::/inline",
            "crates/actix/src/lib.rs::serve_catalog.PUT /inline",
        ),
        // Resources.
        ("http::GET::/orders", "crates/actix/src/orders.rs::list"),
        ("http::POST::/orders", "crates/actix/src/orders.rs::create"),
        (
            "http::PATCH::/orders/{}",
            "crates/actix/src/orders.rs::update",
        ),
        ("http::HEAD::/orders/{}", "crates/actix/src/orders.rs::show"),
        (
            "http::DELETE::/orders/{}",
            "crates/actix/src/orders.rs::item.DELETE /orders/{}",
        ),
        // `.configure(f)`: what `f` registers, with and without a scope around the call.
        ("http::GET::/status", "crates/actix/src/admin.rs::status"),
        (
            "http::GET::/admin/dashboard",
            "crates/actix/src/admin.rs::dashboard",
        ),
        (
            "http::GET::/tools/audit",
            "crates/actix/src/admin.rs::audit",
        ),
        // Written nowhere in its crate but where it is defined: its own path, no prefix. Its
        // name bound (a `let`, a parameter, a field), a reference a local of the name hides and
        // a mention in another crate register nothing. Handlers that code Meada does not read
        // (`services!`, `vec!`, `cfg_if!`, a reference before a `let` of the name) mentions are
        // possibly registered there: served nowhere.
        ("http::GET::/stray/{}", "crates/actix/src/stray.rs::lonely"),
        ("http::GET::/bound", "crates/actix/src/stray.rs::bound"),
        (
            "http::GET::/shadowed",
            "crates/actix/src/stray.rs::shadowed",
        ),
        // A configuration passed on to the function a function calls.
        (
            "http::GET::/more/extras",
            "crates/actix/src/admin.rs::extra",
        ),
    ]);
    assert_eq!(actual, expected);
}

/// The Rocket service of the Rust fixture (spec #30): attribute routes listed by `routes!` under
/// each `mount` prefix, a function that returns the list, `<id>`, `<path..>` and a query part.
/// A prefix read at run time serves nothing, and an unmounted handler is served at its own path.
#[test]
fn rust_rocket_routes_are_providers_under_their_mount() {
    let (_, actual) = providers("rust_rocket", "conformance-rust", "crates/rocket/src/");
    let expected = pairs(&[
        ("http::GET::/", "crates/rocket/src/pages.rs::index"),
        ("http::GET::/about", "crates/rocket/src/pages.rs::about"),
        ("http::GET::/posts", "crates/rocket/src/posts.rs::list"),
        ("http::GET::/posts/{}", "crates/rocket/src/posts.rs::show"),
        ("http::POST::/posts", "crates/rocket/src/posts.rs::create"),
        (
            "http::GET::/posts/search",
            "crates/rocket/src/posts.rs::search",
        ),
        (
            "http::GET::/posts/files/{}",
            "crates/rocket/src/posts.rs::files",
        ),
        (
            "http::GET::/legacy/old/{}",
            "crates/rocket/src/posts.rs::old",
        ),
        ("http::GET::/stray", "crates/rocket/src/stray.rs::lonely"),
    ]);
    assert_eq!(actual, expected);
}

/// Rocket's canonical form (spec #30): `#[macro_use] extern crate rocket;` brings its attributes
/// into every module of the crate, bare.
#[test]
fn rust_rocket_macro_use_brings_the_attributes_into_every_module() {
    let (_, actual) = providers("rust_greeter", "conformance-rust", "crates/greeter/src/");
    let expected = pairs(&[
        ("http::GET::/hello", "crates/greeter/src/main.rs::index"),
        ("http::GET::/hello/{}", "crates/greeter/src/names.rs::named"),
    ]);
    assert_eq!(actual, expected);
}

/// A library whose root is not under `src/` (`[lib] path = "lib.rs"`) reads `#[macro_use]` from
/// that root, as its crate's module tree says.
#[test]
fn rust_macro_use_follows_the_crate_root_wherever_it_is() {
    let (_, actual) = providers("rust_flat", "conformance-rust", "crates/flat/");
    let expected = pairs(&[
        ("http::GET::/flat/page", "crates/flat/pages.rs::page"),
        ("package::cargo::flat", "crates/flat/Cargo.toml"),
    ]);
    assert_eq!(actual, expected);
}

/// A file writing more names than `Facts::mentions` keeps makes only its own crate's handlers
/// possibly registered: one in another crate leaves the unregistered handler at its own path.
#[test]
fn a_file_past_the_mentions_limit_hides_only_its_own_crate() {
    let many: String = (0..20_000).map(|i| format!("    n{i};\n")).collect();
    let big = format!("pub fn big() {{\n{many}}}\n");
    let lonely = "\"http::GET::/stray/{}\"";
    for (krate, served) in [("rocket", true), ("actix", false)] {
        let repo = fixture_repo(
            &format!("rust_mentions_{krate}"),
            "conformance-rust",
            Some(ORIGIN),
        );
        let src = repo.join(format!("crates/{krate}/src"));
        let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
        std::fs::write(src.join("lib.rs"), format!("mod big;\n{lib}")).unwrap();
        std::fs::write(src.join("big.rs"), &big).unwrap();
        let export = Engine::open(&repo).unwrap().export().unwrap();
        assert_eq!(export.contains(lonely), served, "a big file in {krate}");
    }
}

/// The constants and returns the handlers of a call are expanded from share one budget: a list
/// that fits is served under its scope, the same list past the budget is read no further.
#[test]
fn an_actix_list_expanded_past_the_work_budget_is_not_read() {
    let repo = fixture_repo("rust_actix_work", "conformance-rust", Some(ORIGIN));
    let src = repo.join("crates/actix/src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    std::fs::write(src.join("lib.rs"), format!("mod big;\n{lib}")).unwrap();
    let n = 5000;
    let handlers: String = (0..n)
        .map(|i| format!("#[get(\"/h{i}\")]\nasync fn h{i}() {{}}\n"))
        .collect();
    let names: Vec<String> = (0..n).map(|i| format!("h{i}")).collect();
    let big = format!(
        "use actix_web::{{get, web, App}};\n{handlers}const ALL: [u8; {n}] = [{}];\n\
         pub fn a() -> App<()> {{ App::new().service(web::scope(\"/a\").service(ALL)) }}\n\
         pub fn b() -> App<()> {{ App::new().service(web::scope(\"/b\").service(ALL)) }}\n",
        names.join(", ")
    );
    std::fs::write(src.join("big.rs"), big).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(export.contains("http::GET::/a/h0\""));
    assert!(!export.contains("http::GET::/b/h0\""));
}

/// A method router chain longer than the limit read is no provider; the same router with a
/// short chain is.
#[test]
fn an_axum_method_router_chain_past_the_limit_is_not_read() {
    let repo = fixture_repo("rust_axum_chain", "conformance-rust", Some(ORIGIN));
    let src = repo.join("crates/web/src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    std::fs::write(src.join("lib.rs"), format!("mod chain;\n{lib}")).unwrap();
    let layers = ".layer(())".repeat(70);
    let chain = format!(
        "use axum::Router;\nuse axum::routing::get;\n\
         pub fn router() -> Router {{\n    Router::new()\n        .route(\"/short\", get(h).layer(()))\n        \
         .route(\"/long\", get(h){layers})\n}}\nfn h() {{}}\n"
    );
    std::fs::write(src.join("chain.rs"), chain).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(export.contains("http::GET::/short"));
    assert!(!export.contains("http::GET::/long"));
}

/// A chain of thousands of `route` calls is read in one pass and only as far as the limit: the
/// routes near its start are providers, the ones past it are not.
#[test]
fn a_long_axum_chain_is_read_up_to_its_limit() {
    let repo = fixture_repo("rust_axum_long", "conformance-rust", Some(ORIGIN));
    let src = repo.join("crates/web/src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    std::fs::write(src.join("lib.rs"), format!("mod long;\n{lib}")).unwrap();
    let routes: String = (0..3000)
        .map(|i| format!("        .route(\"/r{i}\", get(h))\n"))
        .collect();
    let long = format!(
        "use axum::Router;\nuse axum::routing::get;\n\
         pub fn router() -> Router {{\n    Router::new()\n{routes}}}\nfn h() {{}}\n"
    );
    std::fs::write(src.join("long.rs"), long).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(export.contains("http::GET::/r10\""));
    assert!(!export.contains("http::GET::/r2999\""));
}

/// The consumers of the Rust conformance fixture's `worker` crate in `file` of its `src/`.
fn rust_clients(case: &str, file: &str) -> Vec<Consumer> {
    let repo = fixture_repo(case, "conformance-rust", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json = serde_json::from_str(&export).unwrap();
    consumers(&json, &format!("crates/worker/src/{file}"))
}

#[test]
fn rust_reqwest_functions_are_consumers_and_get_on_other_objects_is_not() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/modules.rs::{function}::{call}");
    let expected = triples(&[
        // A function of the crate, of its `blocking` module imported and written in full.
        ("http::GET::/health", host, &site("health", "reqwest.get#0")),
        (
            "http::GET::/blocking/health",
            host,
            &site("blocking_health", "blocking.get#0"),
        ),
        (
            "http::GET::/blocking/ping",
            host,
            &site("blocking_health", "reqwest.blocking.get#0"),
        ),
        ("http::GET::/items", host, &site("items", "client.get#0")),
    ]);
    // `get` on a type of the crate and on a map is no consumer.
    assert_eq!(rust_clients("rust_http_modules", "modules.rs"), expected);
}

#[test]
fn rust_reqwest_clients_send_with_their_method_name_or_request() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/clients.rs::{function}::{call}");
    let expected = triples(&[
        (
            "http::GET::/verbs/get",
            host,
            &site("verbs", "client.get#0"),
        ),
        (
            "http::POST::/verbs/post",
            host,
            &site("verbs", "client.post#0"),
        ),
        (
            "http::PUT::/verbs/put",
            host,
            &site("verbs", "client.put#0"),
        ),
        (
            "http::PATCH::/verbs/patch",
            host,
            &site("verbs", "client.patch#0"),
        ),
        (
            "http::DELETE::/verbs/delete",
            host,
            &site("verbs", "client.delete#0"),
        ),
        (
            "http::HEAD::/verbs/head",
            host,
            &site("verbs", "client.head#0"),
        ),
        // `Client::builder()...build()?` is a client.
        ("http::GET::/built", host, &site("built", "client.get#0")),
        // `request(Method::X, url)`: the method written in full, through a `use` and a `let`.
        (
            "http::PUT::/requested/put",
            host,
            &site("requested", "client.request#0"),
        ),
        (
            "http::DELETE::/requested/delete",
            host,
            &site("requested", "client.request#1"),
        ),
        (
            "http::PATCH::/requested/patch",
            host,
            &site("requested", "client.request#2"),
        ),
        // A client that is never named, built in the expression.
        (
            "http::GET::/inline",
            host,
            &site("inline", "(call_expression).get#0"),
        ),
        (
            "http::POST::/inline/path",
            host,
            &site("inline", "(call_expression).post#0"),
        ),
        // The blocking client, `use ... as`, and a builder chain called on in place.
        (
            "http::GET::/blocking/get",
            host,
            &site("blocking", "client.get#0"),
        ),
        (
            "http::PATCH::/blocking/request",
            host,
            &site("blocking", "client.request#0"),
        ),
        (
            "http::DELETE::/blocking/built",
            host,
            &site("blocking", "(try_expression).delete#0"),
        ),
    ]);
    assert_eq!(rust_clients("rust_http_clients", "clients.rs"), expected);
}

#[test]
fn rust_ureq_functions_and_agents_are_consumers_with_their_method() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/agents.rs::{function}::{call}");
    let expected = triples(&[
        (
            "http::GET::/u/get",
            host,
            &site("module_functions", "ureq.get#0"),
        ),
        (
            "http::POST::/u/post",
            host,
            &site("module_functions", "ureq.post#0"),
        ),
        (
            "http::PUT::/u/put",
            host,
            &site("module_functions", "ureq.put#0"),
        ),
        (
            "http::PATCH::/u/patch",
            host,
            &site("module_functions", "ureq.patch#0"),
        ),
        (
            "http::DELETE::/u/delete",
            host,
            &site("module_functions", "ureq.delete#0"),
        ),
        (
            "http::HEAD::/u/head",
            host,
            &site("module_functions", "ureq.head#0"),
        ),
        (
            "http::OPTIONS::/u/request",
            host,
            &site("module_functions", "ureq.request#0"),
        ),
        // An `Agent` from `agent()`, from a builder and from `Agent::new()`.
        (
            "http::GET::/agent/get",
            host,
            &site("agents", "agent.get#0"),
        ),
        (
            "http::DELETE::/agent/request",
            host,
            &site("agents", "agent.request#0"),
        ),
        (
            "http::POST::/agent/built",
            host,
            &site("agents", "built.post#0"),
        ),
        (
            "http::PUT::/agent/new",
            host,
            &site("agents", "named.put#0"),
        ),
    ]);
    assert_eq!(rust_clients("rust_http_agents", "agents.rs"), expected);
}

#[test]
fn rust_client_wrappers_are_expanded_through_their_exact_callers() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/wrappers.rs::{function}::{call}");
    let expected = triples(&[
        // One level, and two: `resource` calls `api_get` with a path of its own parameter.
        (
            "http::GET::/wrapped/one",
            host,
            &site("wrapped", "api_get#0"),
        ),
        (
            "http::GET::/resources/tags",
            host,
            &site("wrapped", "resource#0"),
        ),
        (
            "http::GET::/resources/users",
            host,
            &site("wrapped", "resource#1"),
        ),
        // The client is a parameter too: the caller's constructor is the client. One held by a
        // struct (`Api.ping`) is no consumer.
        (
            "http::POST::/param/one",
            host,
            &site("through_a_parameter", "send#0"),
        ),
        // A wrapper that calls itself: the cycle is cut, the outside caller's path is read.
        (
            "http::GET::/recursive/one",
            host,
            &site("recursive", "recurse#0"),
        ),
        // A public function with no caller here keeps the parameter as a hole.
        (
            "http::GET::/open/{}",
            host,
            &site("open_ended", "reqwest.get#0"),
        ),
    ]);
    assert_eq!(rust_clients("rust_http_wrappers", "wrappers.rs"), expected);
}

#[test]
fn rust_values_derived_from_a_client_and_name_matches_are_no_consumers() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/derived.rs::{function}::{call}");
    let expected = triples(&[
        // `headers().get(..)` of a response, `get` on a JSON value and on `client.api()` (a method
        // of a trait the crate defines) are not.
        (
            "http::GET::/derived/real",
            host,
            &site("derived", "client.get#0"),
        ),
        // A client from `factory.make()`, found by name, is not one; a wrapper reached only by
        // a name match keeps its parameter as a hole (the call `sender.send_named("called")` is
        // not its caller).
        (
            "http::GET::/by-name/{}",
            host,
            &site("send_named", "reqwest.get#0"),
        ),
        // A `{base}` that a loop or a closure parameter binds is not the `let base` around it.
        ("http::GET::/loop", "", &site("shadowed", "reqwest.get#0")),
        (
            "http::GET::/closure",
            "",
            &site("shadowed", "reqwest.get#1"),
        ),
        ("http::GET::/outer", "", &site("shadowed", "reqwest.get#2")),
    ]);
    assert_eq!(rust_clients("rust_http_derived", "derived.rs"), expected);
}

#[test]
fn rust_urls_fold_consts_format_concat_and_strings_built_in_the_function() {
    let host = "host:api.example.com";
    let site =
        |function: &str, call: &str| format!("crates/worker/src/urls.rs::{function}::{call}");
    let get = |function: &str, n: usize| site(function, &format!("reqwest.get#{n}"));
    let expected = triples(&[
        // A const of the file, one imported through `use`, one through a path, a `concat!` const,
        // and a static captured in a format string.
        ("http::GET::/local", host, &get("consts", 0)),
        ("http::GET::/users", host, &get("consts", 1)),
        ("http::GET::/", host, &get("consts", 2)),
        ("http::GET::/tags", host, &get("consts", 3)),
        ("http::GET::/base/items", host, &get("consts", 4)),
        // `{base}`, `{}`, `{0}`, `{b}` with `b = base`, `{{` and `concat!`.
        ("http::GET::/fmt/inline", host, &get("formats", 0)),
        ("http::GET::/fmt/positional/1", host, &get("formats", 1)),
        ("http::GET::/fmt/index", host, &get("formats", 2)),
        ("http::GET::/fmt/named", host, &get("formats", 3)),
        ("http::GET::/fmt/escaped/{}", host, &get("formats", 4)),
        ("http::GET::/concat/7", host, &get("formats", 5)),
        // `push_str`, `push` and `+=`, `+`, and a read before a push sees the String as it was.
        ("http::GET::/strings/push/tail", host, &get("strings", 0)),
        ("http::GET::/strings/sum/chain", host, &get("strings", 1)),
        ("http::GET::/early", host, &get("strings", 2)),
        ("http::GET::/early/late", host, &get("strings", 3)),
    ]);
    assert_eq!(rust_clients("rust_http_urls", "urls.rs"), expected);
}

#[test]
fn rust_urls_that_do_not_fold_are_holes_or_unresolved() {
    let host = "host:api.example.com";
    let site = |function: &str| format!("crates/worker/src/dynamic.rs::{function}::reqwest.get#0");
    let expected = triples(&[
        // A call's result, and a String a loop grows, are unreadable: the consumer has no
        // contract. A format spec prints something else than the value: a hole.
        ("unresolved", "", &site("unresolved")),
        ("unresolved", "", &site("looped")),
        ("http::GET::/spec/{}", host, &site("spec")),
    ]);
    assert_eq!(rust_clients("rust_http_dynamic", "dynamic.rs"), expected);
}

#[test]
fn rust_base_urls_from_the_environment_are_the_variable_meada_never_reads() {
    let site =
        |function: &str, n: usize| format!("crates/worker/src/env.rs::{function}::reqwest.get#{n}");
    let expected = triples(&[
        // `std::env::var` in full, `use std::env`, `use std::env::var` inside a function.
        ("http::GET::/std", "env:STD_URL", &site("full_path", 0)),
        (
            "http::GET::/imported",
            "env:IMPORTED_URL",
            &site("imported", 0),
        ),
        (
            "http::GET::/function",
            "env:FUNCTION_URL",
            &site("function_import", 0),
        ),
        // `env!` bound to a name and inside `concat!`, and `dotenvy::var` with `?`.
        ("http::GET::/macro", "env:MACRO_URL", &site("macros", 0)),
        ("http::GET::/concat", "env:CONCAT_URL", &site("macros", 1)),
        ("http::GET::/dotenv", "env:DOTENV_URL", &site("dotenv", 0)),
        // A fallback and string methods keep the one read.
        ("http::GET::/or", "env:OR_URL", &site("fallbacks", 0)),
        ("http::GET::/else", "env:ELSE_URL", &site("fallbacks", 1)),
        ("http::GET::/trim", "env:TRIM_URL", &site("fallbacks", 2)),
        // A second read in the fallback leaves a hole, and `option_env!` is not read.
        ("http::GET::/two", "", &site("two_reads", 0)),
        ("http::GET::/macro-fallback", "", &site("two_reads", 1)),
        ("http::GET::/optional", "", &site("optional", 0)),
        // A read in the middle of the URL is a hole in the path, not the base.
        (
            "http::GET::/{}/mid",
            "host:api.example.com",
            &site("mid_url", 0),
        ),
    ]);
    assert_eq!(rust_clients("rust_http_env", "env.rs"), expected);
}

#[test]
fn rust_calls_spelled_like_an_environment_read_but_not_one_are_not_read() {
    let site2 = |function: &str, n: usize| {
        format!("crates/worker/src/not_env.rs::{function}::reqwest.get#{n}")
    };
    let site = |function: &str| site2(function, 0);
    let expected = triples(&[
        // An `env` the file defines, and `env::var` with no `use std::env` in scope: the base is a
        // hole, not a variable.
        ("http::GET::/own", "", &site("own_module")),
        ("http::GET::/bare", "", &site("other.bare")),
        // A parameter or a closure named `var` hides `use std::env::var`.
        ("http::GET::/parameter", "", &site("hidden.parameter")),
        ("http::GET::/closure", "", &site("hidden.closure")),
        // A `var`, an `env!` and a `format!` imported from a path outside `std`.
        ("http::GET::/foreign", "", &site("foreign.read")),
        ("http::GET::/macro", "", &site2("foreign.read", 1)),
        ("unresolved", "", &site2("foreign.read", 2)),
    ]);
    assert_eq!(rust_clients("rust_http_not_env", "not_env.rs"), expected);
}

/// A read under hundreds of string methods is read only as far as `VALUE_DEPTH`, and a read with
/// hundreds of fallbacks nested in it is a hole: the base is no variable, and indexing carries on.
#[test]
fn an_environment_read_under_a_deep_chain_is_a_hole() {
    let repo = fixture_repo("rust_env_long", "conformance-rust", Some(ORIGIN));
    let src = repo.join("crates/worker/src");
    let lib = std::fs::read_to_string(src.join("lib.rs")).unwrap();
    std::fs::write(src.join("lib.rs"), format!("mod long_env;\n{lib}")).unwrap();
    let trims = ".trim()".repeat(200);
    let nested = format!(
        "{}\"/x\"{}",
        "std::env::var(\"A\").unwrap_or(".repeat(200),
        ")".repeat(200)
    );
    let long = format!(
        "pub async fn chain() {{\n    let a = std::env::var(\"LONG_URL\").unwrap(){trims}.to_string();\n    \
         let _ = reqwest::get(a + \"/long\").await;\n    \
         let b = {nested};\n    let _ = reqwest::get(b + \"/nested\").await;\n}}\n"
    );
    std::fs::write(src.join("long_env.rs"), long).unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json = serde_json::from_str(&export).unwrap();
    let found = consumers(&json, "crates/worker/src/long_env.rs");
    let reads: Vec<_> = found
        .iter()
        .map(|(c, v, _)| (c.as_str(), v.as_str()))
        .collect();
    assert_eq!(
        reads,
        [("http::GET::/long", ""), ("http::GET::/nested", "")]
    );
}

/// A crate's exports are the `pub` items reachable from its root through `pub` modules and
/// `pub use`; `pub(crate)` and narrower items stay inside it (the `exports` table, read as the
/// next ticket's cross-repository imports do).
#[test]
fn rust_exports_are_the_public_items_reachable_from_the_root() {
    let repo = fixture_repo("conformance_rust_exports", "conformance-rust", Some(ORIGIN));
    Engine::open(&repo).unwrap().export().unwrap();
    let db = rusqlite::Connection::open(repo.join(".meada/index.db")).unwrap();
    let mut rows = db
        .prepare(
            "SELECT path, name, file, target FROM exports WHERE path LIKE 'crates/%' \
             OR path LIKE 'api/%' ORDER BY path, name",
        )
        .unwrap();
    let found: Vec<String> = rows
        .query_map([], |row| {
            let col = |n| {
                row.get::<_, Option<String>>(n)
                    .map(|v| v.unwrap_or("-".into()))
            };
            Ok(format!("{} {} {} {}", col(0)?, col(1)?, col(2)?, col(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect();
    // `api` has binaries alone (`src/main.rs`, `src/bin/`, a `[[bin]]`): no other crate can
    // import them, so nothing in it is exported, whatever its `pub`s and modules say.
    let expected = [
        // `pub use` at the root, `pub` modules and the items of a `pub` module written in
        // place; a private module and what it holds are not.
        "crates/actix/src/lib.rs serve_catalog crates/actix/src/lib.rs serve_catalog",
        "crates/billing/src/billing.rs Invoice crates/billing/src/invoice.rs Invoice",
        "crates/billing/src/billing.rs copies crates/billing/src/billing.rs copies",
        "crates/billing/src/billing.rs invoice crates/billing/src/invoice.rs -",
        "crates/billing/src/billing.rs settle crates/billing/src/billing.rs settle",
        "crates/billing/src/billing.rs total crates/billing/src/billing.rs total",
        "crates/billing/src/invoice.rs Invoice crates/billing/src/invoice.rs Invoice",
        "crates/billing/src/invoice.rs tax crates/billing/src/invoice.rs tax",
        "crates/flat/lib.rs serve crates/flat/lib.rs serve",
        "crates/ledger-core/src/lib.rs Ledger crates/ledger-core/src/lib.rs Ledger",
        "crates/ledger-core/src/lib.rs amounts crates/ledger-core/src/lib.rs amounts",
        "crates/ledger-core/src/lib.rs entries crates/ledger-core/src/lib.rs entries",
        "crates/ledger-core/src/lib.rs entries.add crates/ledger-core/src/lib.rs entries.add",
        // A `pub use` in a `pub` module written in place is one of its members.
        "crates/ledger-core/src/lib.rs entries.secret crates/ledger-core/src/lib.rs internal.secret",
        // `mod meter; pub use meter::meter;`: the private module's file exports nothing, and
        // the root gives the item.
        "crates/ledger-core/src/lib.rs meter crates/ledger-core/src/meter.rs meter",
        "crates/ledger-core/src/lib.rs post crates/ledger-core/src/lib.rs post",
        "crates/scratch/src/lib.rs ignored crates/scratch/src/lib.rs ignored",
        "crates/scratch/src/lib.rs reach crates/scratch/src/lib.rs reach",
        "crates/web/src/admin.rs router crates/web/src/admin.rs router",
        "crates/web/src/items.rs create crates/web/src/items.rs create",
        "crates/web/src/items.rs list crates/web/src/items.rs list",
        "crates/web/src/items.rs remove crates/web/src/items.rs remove",
        "crates/web/src/items.rs router crates/web/src/items.rs router",
        "crates/web/src/items.rs show crates/web/src/items.rs show",
        "crates/web/src/items.rs update crates/web/src/items.rs update",
        "crates/web/src/lib.rs Builder crates/web/src/lib.rs Builder",
        "crates/web/src/lib.rs Holder crates/web/src/lib.rs Holder",
        "crates/web/src/lib.rs admin crates/web/src/admin.rs -",
        "crates/web/src/lib.rs app crates/web/src/lib.rs app",
        "crates/web/src/lib.rs hop crates/web/src/lib.rs hop",
        "crates/web/src/lib.rs items crates/web/src/items.rs -",
        "crates/web/src/lib.rs reassigned crates/web/src/lib.rs reassigned",
        "crates/web/src/lib.rs rebuilt crates/web/src/lib.rs rebuilt",
        "crates/web/src/lib.rs served crates/web/src/lib.rs served",
        "crates/web/src/lib.rs unread crates/web/src/lib.rs unread",
        "crates/web/src/lib.rs wrapper crates/web/src/lib.rs wrapper",
    ];
    assert_eq!(found, expected);
}

/// The Python conformance fixture's export, and its providers (contract, site) whose site is
/// under `dir`, sorted.
fn python_providers(case: &str, dir: &str) -> (Value, Vec<(String, String)>) {
    providers(case, "conformance-python", dir)
}

/// The export of `fixture`, and its providers (contract, site) whose site is under `dir`,
/// sorted.
fn providers(case: &str, fixture: &str, dir: &str) -> (Value, Vec<(String, String)>) {
    let repo = fixture_repo(case, fixture, Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let mut providers: Vec<(String, String)> = json["providers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let site = p["site"].as_str().unwrap();
            let site = site.strip_prefix(&format!("{REPO}::")).unwrap();
            (p["contract"].as_str().unwrap().into(), site.into())
        })
        .filter(|(_, site): &(String, String)| site.starts_with(dir))
        .collect();
    providers.sort();
    (json, providers)
}

/// `(contract, site)` pairs as owned strings, sorted.
fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = (pairs.iter())
        .map(|(c, s)| (c.to_string(), s.to_string()))
        .collect();
    out.sort();
    out
}

/// A consumer of the export: its contract (`unresolved` for none), how it names the provider's
/// repository (empty for none) and its site.
type Consumer = (String, String, String);

/// The consumers of `json` whose site is under `dir`, sorted, with repository-relative sites.
fn consumers(json: &Value, dir: &str) -> Vec<Consumer> {
    let mut found: Vec<Consumer> = json["consumers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let site = c["site"].as_str().unwrap();
            let site = site.strip_prefix(&format!("{REPO}::")).unwrap();
            (
                c["contract"].as_str().unwrap_or("unresolved").into(),
                c["via"].as_str().unwrap_or("").into(),
                site.into(),
            )
        })
        .filter(|(.., site): &Consumer| site.starts_with(dir))
        .collect();
    found.sort();
    found
}

/// `(contract, via, site)` as owned strings, sorted.
fn triples(items: &[(&str, &str, &str)]) -> Vec<Consumer> {
    let mut out: Vec<Consumer> = (items.iter())
        .map(|(c, v, s)| (c.to_string(), v.to_string(), s.to_string()))
        .collect();
    out.sort();
    out
}

#[test]
fn python_fastapi_and_flask_routes_are_providers_under_every_prefix() {
    let (json, actual) = python_providers("python_http_providers", "web/");
    let expected = [
        // FastAPI: method decorators and `api_route` on the app.
        ("http::GET::/health", "web/api.py::health"),
        ("http::GET::/ping", "web/api.py::ping"),
        ("http::POST::/ping", "web/api.py::ping"),
        ("http::GET::/echo", "web/api.py::echo"),
        // One router under three prefixes: nested in `v1` (its own prefix after the one given
        // when including it), directly, and through a function that returns its argument.
        (
            "http::GET::/api/v1/items/catalog/{}",
            "web/items.py::read_item",
        ),
        (
            "http::POST::/api/v1/items/catalog",
            "web/items.py::create_item",
        ),
        ("http::GET::/legacy/catalog/{}", "web/items.py::read_item"),
        ("http::POST::/legacy/catalog", "web/items.py::create_item"),
        ("http::GET::/relay/catalog/{}", "web/items.py::read_item"),
        ("http::POST::/relay/catalog", "web/items.py::create_item"),
        // A router returned by a function and included by another one, as arguments.
        (
            "http::DELETE::/internal/admin/users/{}",
            "web/factory.py::build_admin.remove_user",
        ),
        // Flask: `route` without methods serves GET; a Blueprint's own prefix, and one replaced
        // on registration. The `get` shortcut yields nothing.
        ("http::GET::/", "web/pages.py::index"),
        ("http::GET::/orders/{}", "web/pages.py::order"),
        ("http::DELETE::/orders/{}", "web/pages.py::order"),
        ("http::GET::/reports/daily/{}", "web/pages.py::daily"),
        ("http::PUT::/members/{}", "web/accounts.py::update"),
        // A prefix or a path with a part that cannot be read yields no route; a path
        // parameter written in the literal stays one.
        ("http::GET::/partial/{}", "web/partial.py::read_part"),
    ];
    assert_eq!(actual, pairs(&expected));
    // A Python function named `fetch` is no HTTP consumer.
    assert_eq!(consumers(&json, "web/"), []);
}

#[test]
fn python_django_urlconfs_are_providers_with_the_methods_django_allows() {
    let (_, actual) = python_providers("python_django_providers", "shop/");
    let expected = [
        // Unrestricted function views serve every method.
        ("http::ANY::/health", "shop/views.py::health"),
        // `urlpatterns += [...]` and `urlpatterns = urlpatterns + [...]` extend the list; the
        // `static(...)` added between them is left out and voids nothing.
        ("http::ANY::/about", "shop/views.py::about"),
        ("http::ANY::/contact", "shop/views.py::contact"),
        // A `re_path()` with a simple named group; the one beyond them yields nothing, and so
        // does a route with a part that cannot be read.
        ("http::ANY::/vintage/{}", "shop/views.py::vintage"),
        // `include()` by dotted name, two levels deep: `require_http_methods`, a class-based
        // view's own handler methods, and `require_safe` reached through its module.
        ("http::GET::/shop/orders/{}", "shop/orders/views.py::order"),
        (
            "http::DELETE::/shop/orders/{}",
            "shop/orders/views.py::order",
        ),
        (
            "http::GET::/shop/orders",
            "shop/orders/views.py::OrderList.get",
        ),
        (
            "http::POST::/shop/orders",
            "shop/orders/views.py::OrderList.post",
        ),
        (
            "http::GET::/shop/items/{}/stock",
            "shop/orders/items.py::stock",
        ),
        (
            "http::HEAD::/shop/items/{}/stock",
            "shop/orders/items.py::stock",
        ),
        // `include()` of an imported module: `require_GET` and `require_POST`.
        (
            "http::GET::/catalog/products",
            "shop/catalog/urls.py::products",
        ),
        (
            "http::POST::/catalog/upload",
            "shop/catalog/urls.py::upload",
        ),
    ];
    assert_eq!(actual, pairs(&expected));
}

#[test]
fn python_django_urlconfs_an_include_may_name_are_not_served_at_the_root() {
    let (_, actual) = providers("python_django_includes", "python-django", "");
    let expected = [
        ("http::ANY::/health", "mysite/views.py::health"),
        // `include()` of a tuple, holding a module's dotted name or a list of entries.
        ("http::ANY::/shop/items", "shop/views.py::items"),
        ("http::ANY::/extra/ping", "mysite/views.py::ping"),
        // `apps/vendor/urls.py` may be what `include("vendor.urls")` names: no `/deals`.
    ];
    assert_eq!(actual, pairs(&expected));

    // An `include()` whose argument cannot be read may name any URLconf: none is a root.
    let repo = fixture_repo("python_django_unknown", "python-django", Some(ORIGIN));
    let plugins = "from django.urls import include, path\nfrom registry import load\n\n\
                   urlpatterns = [path(\"plugins/\", include(load()))]\n";
    std::fs::write(repo.join("apps/plugins.py"), plugins).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    assert_eq!(json["providers"], serde_json::json!([]));
}

/// The leaf routes of a ladder of `levels` URLconfs, each including the next under each of
/// `prefixes`.
fn django_ladder(case: &str, levels: usize, prefixes: &[&str]) -> Vec<String> {
    let repo = fixture_repo(case, "python-django", Some(ORIGIN));
    let dir = repo.join("ladder");
    std::fs::create_dir(&dir).unwrap();
    std::fs::write(dir.join("__init__.py"), "").unwrap();
    std::fs::write(dir.join("views.py"), "def leaf(request):\n    pass\n").unwrap();
    for n in 0..levels {
        let next = n + 1;
        let include =
            |prefix: &str| format!("    path(\"{prefix}/\", include(\"ladder.l{next}\")),\n");
        let src = format!(
            "from django.urls import include, path\n\nurlpatterns = [\n{}]\n",
            prefixes.iter().map(|p| include(p)).collect::<String>()
        );
        std::fs::write(dir.join(format!("l{n}.py")), src).unwrap();
    }
    let leaf = "from django.urls import path\n\nfrom ladder.views import leaf\n\n\
                urlpatterns = [path(\"x\", leaf)]\n";
    std::fs::write(dir.join(format!("l{levels}.py")), leaf).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let site = format!("{REPO}::ladder/views.py::leaf");
    (json["providers"].as_array().unwrap().iter())
        .filter(|p| p["site"] == site.as_str())
        .map(|p| p["contract"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn python_django_includes_are_read_to_a_budget() {
    // A module included under two prefixes serves its routes under both.
    let mut routes = django_ladder("python_django_diamond", 2, &["a", "b"]);
    routes.sort();
    assert_eq!(
        routes,
        ["/a/a/x", "/a/b/x", "/b/a/x", "/b/b/x"].map(|p| format!("http::ANY::{p}"))
    );
    // A ladder of 24 levels has 2^24 paths: the budget stops the walk well before them.
    let routes = django_ladder("python_django_ladder", 24, &["a", "b"]);
    assert!(!routes.is_empty());
    assert!(routes.len() < 1 << 20, "{}", routes.len());
}

#[test]
fn python_django_includes_are_followed_to_a_depth() {
    // `INCLUDE_DEPTH` (64) includes, each of the next module.
    let chain =
        |levels: usize| django_ladder(&format!("python_django_chain_{levels}"), levels, &["a"]);
    assert_eq!(chain(64), [format!("http::ANY::{}/x", "/a".repeat(64))]);
    assert_eq!(chain(65), Vec::<String>::new());
}

#[test]
fn python_routers_whose_mount_is_not_seen_are_not_served_at_the_root() {
    let (_, actual) = providers("python_mounts", "python-mounts", "");
    let expected = [
        // The apps, at their root, and a sub-application under the path that mounts it.
        ("http::GET::/health", "backend/app/main.py::health"),
        ("http::GET::/v1/ping", "backend/app/main.py::ping"),
        ("http::GET::/about", "backend/app/site.py::about"),
        // The users router and the pages Blueprint are mounted through imports that do not
        // resolve, and `legacy` may be what an unreadable mount puts under `/legacy`: none of
        // them is served at the root.
    ];
    assert_eq!(actual, pairs(&expected));
}

/// The levels of `checkout`'s two calls in `python-roots` under a given `pyproject.toml`: the
/// call into `lib/shop` and the call into `src/other`.
fn python_roots(case: &str, pyproject: &str) -> [Option<String>; 2] {
    let repo = fixture_repo(case, "python-roots", Some(ORIGIN));
    std::fs::write(repo.join("pyproject.toml"), pyproject).unwrap();
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    ["total", "helper"].map(|callee| {
        let id = format!("{REPO}::app.py::checkout::{callee}#0");
        let calls = json["calls"].as_array().unwrap();
        let call = calls.iter().find(|c| c["id"] == id).unwrap();
        call["level"].as_str().map(String::from)
    })
}

#[test]
fn python_source_roots_come_from_pyproject() {
    let exact = Some("exact".to_string());
    // Nothing declared: `src` is a root, `lib` is not.
    assert_eq!(python_roots("python_roots_none", ""), [None, exact.clone()]);
    // A `package-dir` whose directory is not named after the package, even by its last
    // letters, says nothing about a root.
    assert_eq!(
        python_roots(
            "python_roots_package_dir_suffix",
            "[tool.setuptools]\npackage-dir = { shop = \"libshop\" }\n",
        ),
        [None, exact.clone()]
    );
    // A declared root replaces `src`, in each of the forms.
    for (case, pyproject) in [
        (
            "python_roots_package_dir_dotted",
            "[tool.setuptools.package-dir]\n\"shop.cart\" = \"lib/shop/cart\"\n",
        ),
        (
            "python_roots_package_dir",
            "[tool.setuptools.package-dir]\n\"\" = \"lib\"\n",
        ),
        (
            "python_roots_package_dir_named",
            "[tool.setuptools]\npackage-dir = { shop = \"./lib/shop\" }\n",
        ),
        (
            "python_roots_find",
            "[tool.setuptools.packages.find]\nwhere = [\"lib\"]\n",
        ),
        (
            "python_roots_poetry",
            "[tool.poetry]\npackages = [{ include = \"shop\", from = \"lib\" }]\n",
        ),
    ] {
        assert_eq!(
            python_roots(case, pyproject),
            [exact.clone(), None],
            "{case}"
        );
    }
}

#[test]
fn python_class_families_are_linearized_once() {
    // A ladder of diamonds: walking every path through it would take 2^24 steps.
    let repo = fixture_repo("python_ladder", "python-roots", Some(ORIGIN));
    let mut src =
        String::from("class L0A:\n    def m(self):\n        return 0\n\n\nclass L0B:\n    pass\n");
    for n in 1..=24 {
        for side in ["A", "B"] {
            let p = n - 1;
            src += &format!("\n\nclass L{n}{side}(L{p}A, L{p}B):\n    pass\n");
        }
    }
    src += "\n\nclass Top(L24A, L24B):\n    def go(self):\n        return self.m()\n";
    std::fs::write(repo.join("ladder.py"), src).unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();

    let id = format!("{REPO}::ladder.py::Top.go::self.m#0");
    let call = json["calls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == id)
        .unwrap();
    assert_eq!(
        (call["level"].as_str(), call["target"].as_str()),
        (
            Some("exact"),
            Some(format!("{REPO}::ladder.py::L0A.m").as_str())
        )
    );
}

#[test]
fn python_pyproject_that_cannot_be_read_declares_no_roots() {
    let exact = Some("exact".to_string());
    // Hostile input: a file that is not TOML, or one past the size limit, still indexes, as
    // a pyproject that declares nothing (`src` is a root, `lib` is not).
    assert_eq!(
        python_roots("python_roots_invalid", "[tool\n"),
        [None, exact.clone()]
    );
    let large = format!(
        "[tool.setuptools.package-dir]\n\"\" = \"lib\"\n# {}\n",
        "x".repeat(1 << 20)
    );
    assert_eq!(python_roots("python_roots_large", &large), [None, exact]);
}

/// The consumers of the Python conformance fixture's export in `file` of `clients/`.
fn python_clients(case: &str, file: &str) -> Vec<Consumer> {
    let repo = fixture_repo(case, "conformance-python", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json = serde_json::from_str(&export).unwrap();
    consumers(&json, &format!("clients/{file}"))
}

#[test]
fn python_requests_and_httpx_functions_are_consumers_with_their_evaluated_path() {
    let host = "host:api.example.com";
    let site = |function: &str, call: &str| format!("clients/modules.py::{function}::{call}");
    let expected = triples(&[
        // A literal and an imported constant joined with `+`.
        (
            "http::GET::/users",
            host,
            &site("list_users", "requests.get#0"),
        ),
        // An f-string holding an imported constant; a keyword argument it does not read.
        (
            "http::POST::/users",
            host,
            &site("create_user", "httpx.post#0"),
        ),
        // An f-string hole that does not fold is a parameter marker.
        (
            "http::PUT::/users/{}/name",
            host,
            &site("rename_user", "requests.put#0"),
        ),
        // A function imported under another name.
        ("http::GET::/status", host, &site("status", "fetch_page#0")),
        // `request(method, url)`, positionally and by keyword.
        (
            "http::DELETE::/cache",
            host,
            &site("purge", "requests.request#0"),
        ),
        (
            "http::PATCH::/cache/flags",
            host,
            &site("purge", "httpx.request#0"),
        ),
        // A module's attribute, joined with `+` and inside an f-string.
        (
            "http::GET::/orders",
            host,
            &site("list_orders", "requests.get#0"),
        ),
        (
            "http::GET::/orders/{}",
            host,
            &site("read_order", "requests.get#0"),
        ),
    ]);

    // `get` on a parameter or on another object (`lookup`) is no consumer.
    assert_eq!(
        python_clients("python_http_modules", "modules.py"),
        expected
    );
}

#[test]
fn python_client_methods_take_the_base_url_of_the_client_they_trace_back_to() {
    let host = "host:api.example.com";
    let site = |function: &str, call: &str| format!("clients/sessions.py::{function}::{call}");
    let expected = triples(&[
        // A session in the module scope, and one assigned in the same scope.
        (
            "http::GET::/session",
            host,
            &site("from_module", "session.get#0"),
        ),
        ("http::POST::/orders", host, &site("from_scope", "s.post#0")),
        // `base_url=` joins in front of a relative path; an absolute URL ignores it.
        (
            "http::GET::/v2/items",
            host,
            &site("with_base", "client.get#0"),
        ),
        (
            "http::OPTIONS::/probe",
            "host:other.example.com",
            &site("with_base", "client.options#0"),
        ),
        // Without one the host stays unresolved.
        ("http::HEAD::/ping", "", &site("no_base", "client.head#0")),
        // Bound by `with` and `async with`, a trailing `/` of the base or none leading the path.
        (
            "http::DELETE::/items/7",
            host,
            &site("bound_by_with", "client.delete#0"),
        ),
        (
            "http::PATCH::/items/9",
            host,
            &site("bound_by_async_with", "client.patch#0"),
        ),
        (
            "http::HEAD::/aio/head",
            host,
            &site("bound_by_async_with", "http.request#0"),
        ),
        (
            "http::GET::/aio",
            host,
            &site("bound_by_async_with", "http.get#0"),
        ),
        // Assigned to a `self` attribute in the class.
        (
            "http::GET::/gateway/health",
            host,
            &site("Gateway.health", "self.http.get#0"),
        ),
        // A parameter counts through the client a caller passes it.
        (
            "http::PUT::/handed/passed",
            host,
            &site("passed", "client.put#0"),
        ),
    ]);

    // The annotated parameter, `Cache.get` and the response's `get` are no consumers.
    assert_eq!(
        python_clients("python_http_sessions", "sessions.py"),
        expected
    );
}

#[test]
fn python_client_wrappers_make_each_call_site_a_consumer_through_levels_and_a_cycle() {
    let host = "host:api.example.com";
    let site = |function: &str, call: &str| format!("clients/wrappers.py::{function}::{call}");
    let expected = triples(&[
        // Two wrapper levels down, the path by position and by keyword.
        ("http::GET::/orders", host, &site("list_orders", "get#0")),
        (
            "http::GET::/orders/kw",
            host,
            &site("list_by_keyword", "get#0"),
        ),
        // The method comes from the wrapper and a hole in the path stays a parameter marker.
        (
            "http::DELETE::/orders/{}",
            host,
            &site("drop_order", "remove#0"),
        ),
        // Through the cycle: `retry` reaches `get` without looping.
        (
            "http::GET::/orders/slow",
            host,
            &site("list_patiently", "retry#0"),
        ),
        // A method wrapper over a client with a base URL.
        (
            "http::GET::/api/users/me",
            host,
            &site("Api.profile", "self.user#0"),
        ),
        // A method wrapper called on an instance of its class; a call that only matches its
        // name (`not_a_client`) is no consumer.
        (
            "http::GET::/stock",
            host,
            &site("stock", "inventory.fetch_item#0"),
        ),
        // Called on the class: the instance passed explicitly takes the receiver, a class
        // method's receiver is the class.
        (
            "http::GET::/pages",
            host,
            &site("explicit_instance", "Pages.load#0"),
        ),
        (
            "http::GET::/shared",
            host,
            &site("class_method", "Pages.load_shared#0"),
        ),
    ]);

    // `Reports.pull` called on a receiver of unknown class (`unrelated`) is a name match: its
    // `client.get` is no consumer.
    assert_eq!(
        python_clients("python_http_wrappers", "wrappers.py"),
        expected
    );
}

#[test]
fn python_wrapper_defaults_fill_the_arguments_a_caller_leaves_out() {
    let site = |function: &str, call: &str| format!("clients/defaults.py::{function}::{call}");
    let expected = triples(&[
        // Method and base both from the defaults.
        (
            "http::GET::/defaults",
            "host:api.example.com",
            &site("list_defaults", "request#0"),
        ),
        // The method by position; the base still from its default.
        (
            "http::POST::/defaults",
            "host:api.example.com",
            &site("create_default", "request#0"),
        ),
        // The base by keyword; the method from its default.
        (
            "http::GET::/other",
            "host:other.example.com",
            &site("other_base", "request#0"),
        ),
        // Keyword-only: the default when left out, the argument when given.
        (
            "http::PUT::/kw-default",
            "host:api.example.com",
            &site("call_keyword_only", "keyword_only#0"),
        ),
        (
            "http::PATCH::/kw-override",
            "host:api.example.com",
            &site("override_keyword_only", "keyword_only#0"),
        ),
        // Positional arguments past the named ones go to `*extra`, never to `method`.
        (
            "http::HEAD::/splat",
            "host:api.example.com",
            &site("call_with_splats", "with_splats#0"),
        ),
    ]);

    assert_eq!(
        python_clients("python_http_defaults", "defaults.py"),
        expected
    );
}

#[test]
fn python_calls_whose_path_cannot_be_evaluated_are_unresolved_consumers() {
    let site = |function: &str, call: &str| format!("clients/dynamic.py::{function}::{call}");
    let expected = triples(&[
        // `str.format`, `%` and a function's result are not folded.
        ("unresolved", "", &site("formatted", "requests.get#0")),
        ("unresolved", "", &site("percent", "requests.delete#0")),
        ("unresolved", "", &site("computed", "requests.post#0")),
        // A method no caller gives: the host is still known.
        (
            "unresolved",
            "host:api.example.com",
            &site("unknown_method", "requests.request#0"),
        ),
        // An environment read is the base, named by its variable.
        (
            "http::GET::/orders",
            "env:ORDERS_URL",
            &site("from_environment", "requests.get#0"),
        ),
    ]);

    assert_eq!(
        python_clients("python_http_dynamic", "dynamic.py"),
        expected
    );
}

#[test]
fn python_environment_reads_name_the_base_url_variable() {
    let site = |function: &str, call: &str| format!("clients/environment.py::{function}::{call}");
    let orders = "http::GET::/orders";
    let env = "env:ORDERS_URL";
    let expected = triples(&[
        // `os.environ[...]`, `os.environ.get(...)` and `os.getenv(...)`, also imported from `os`.
        (orders, env, &site("subscript", "requests.get#0")),
        (orders, env, &site("imported_subscript", "requests.get#0")),
        (orders, env, &site("environ_get", "requests.get#0")),
        (orders, env, &site("getenv_call", "requests.get#0")),
        (orders, env, &site("imported_getenv", "requests.get#0")),
        // `or` fallbacks and default arguments: the variable, whatever its value would be.
        (orders, env, &site("fallback", "requests.get#0")),
        (orders, env, &site("default_argument", "requests.get#0")),
        (
            orders,
            env,
            &site("default_in_environ_get", "requests.get#0"),
        ),
        // String methods, a module name, an f-string hole and a client's `base_url`.
        (orders, env, &site("string_methods", "requests.get#0")),
        (orders, env, &site("named", "requests.get#0")),
        (orders, env, &site("interpolated", "requests.get#0")),
        (orders, env, &site("client_base_url", "client.get#0")),
        // Two variables, or a name only known at run time, name no base.
        (orders, "", &site("two_variables", "requests.get#0")),
        (orders, "", &site("default_reads_another", "requests.get#0")),
        (orders, "", &site("computed_name", "requests.get#0")),
    ]);

    assert_eq!(
        python_clients("python_http_environment", "environment.py"),
        expected
    );
}

#[test]
fn express_router_passed_to_a_function_found_only_by_name_is_no_provider() {
    let repo = fixture_repo("express_by_name", "ts-basic", Some(ORIGIN));
    let providers = |file: &str, src: &str| -> Vec<String> {
        std::fs::write(repo.join(file), src).unwrap();
        let export = Engine::open(&repo).unwrap().export().unwrap();
        let json: Value = serde_json::from_str(&export).unwrap();
        (json["providers"].as_array().unwrap().iter())
            .map(|p| p["contract"].as_str().unwrap().to_string())
            .collect()
    };
    let plugin = "import express from 'express';\n\
        export class Plugin {\n  install(router) { router.get('/plugin', handler); }\n}\n\
        function handler(req, res) {}\n\
        export function unknown(registry) {\n  const app = express();\n  registry.install(app);\n}\n";
    let found = providers("src/plugin.ts", plugin);
    assert!(
        !found.iter().any(|c| c == "http::GET::/plugin"),
        "{found:?}"
    );

    // A receiver of known class is an exact call: the router flows.
    let known = plugin.replace(
        "  install(router)",
        "  run() { this.install(express()); }\n  install(router)",
    );
    let found = providers("src/plugin.ts", &known);
    assert!(found.iter().any(|c| c == "http::GET::/plugin"), "{found:?}");

    // A bare call no import binds, matched only by the function's name, may reach any function.
    providers(
        "src/mount.ts",
        "export function mount(router) { router.get('/mounted', handler); }\n\
         function handler(req, res) {}\n",
    );
    let found = providers(
        "src/main.ts",
        "import express from 'express';\nmount(express());\n",
    );
    assert!(
        !found.iter().any(|c| c == "http::GET::/mounted"),
        "{found:?}"
    );
}

/// The kafkajs forms of the TS/JS pack (spec #20): every producer and consumer of the
/// fixture, as `(consumer, producer, channel)`, all `exact`. `transport.send` and a client of
/// another package produce nothing.
#[test]
fn typescript_and_javascript_kafkajs() {
    let repo = fixture_repo("conformance_kafkajs", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .filter(|l| {
            l["reason"]
                .as_str()
                .unwrap()
                .starts_with("channel::kafka::")
        })
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .filter(|(_, _, reason, _)| reason.starts_with("channel::kafka::"))
        .collect();

    let consume = "src/messages/consume.ts::";
    let produce = "src/messages/produce.ts::";
    let link = |consumer: &str, producer: &str, channel: &str| {
        (
            consumer.to_string(),
            format!("{produce}{producer}"),
            format!("channel::kafka::{channel}"),
            "exact",
        )
    };
    let orders = format!("{consume}consumeOrders.kafka orders.created orders.shipped");
    let mut expected = vec![
        link(&orders, "produceAll::producer.send#1", "orders.created"),
        link(&orders, "produceAll::producer.send#2", "orders.shipped"),
        link(
            &format!("{consume}consumeRefunds.kafka refunds"),
            "emitRefund::emit#0",
            "refunds",
        ),
        link(
            &format!("{consume}listenRetries"),
            "retry::relay#0",
            "retries",
        ),
        link(
            "src/messages/handlers.ts::onPayment",
            "produceAll::producer.send#0",
            "payments",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
}

/// The SQS and NATS forms of the TS/JS pack (spec #20): the AWS SDK v3 commands, `sqs-consumer`
/// and the `nats` connection, as `(consumer, producer, reason, level)`. A client of another
/// package and `publish` on any object produce nothing; NATS wildcards link at `inferred`.
#[test]
fn typescript_and_javascript_sqs_and_nats() {
    let repo = fixture_repo("conformance_sqs_nats", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let mut links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .filter(|l| {
            let reason = l["reason"].as_str().unwrap();
            reason.starts_with("channel::sqs::") || reason.starts_with("channel::nats::")
        })
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    links.sort();

    let sqs = "src/messages/sqs.ts::";
    let nats = "src/messages/nats.ts::";
    let queue = |consumer: &str, producer: &str, name: &str| {
        let (consumer, producer) = (format!("{sqs}{consumer}"), format!("{sqs}{producer}"));
        (consumer, producer, format!("channel::sqs::{name}"), "exact")
    };
    let subject = |consumer: &str, producer: &str, subject: &str, pattern: Option<&str>| {
        let (consumer, producer) = (format!("{nats}{consumer}"), format!("{nats}{producer}"));
        match pattern {
            Some(pattern) => (
                consumer,
                producer,
                format!("channel::nats::{subject} wildcard {pattern}"),
                "inferred",
            ),
            None => (
                consumer,
                producer,
                format!("channel::nats::{subject}"),
                "exact",
            ),
        }
    };
    let created = "publishAll::nc.publish#0";
    let created_eu = "publishAll::nc.publish#1";
    let created_audit = "publishAll::other.publish#0";
    let mut expected = vec![
        queue("receiveJobs", "sendJob::SendMessageCommand#0", "jobs"),
        queue(
            "consumeMail.sqs mail",
            "sendJob::SendMessageCommand#1",
            "mail",
        ),
        queue(
            "onAudit",
            "sendThroughNamespace::aws.SendMessageCommand#0",
            "audit",
        ),
        queue("receiveBilling", "enqueueBilling::enqueue#0", "billing"),
        subject("subscribeOne", created, "jobs.created", Some("jobs.*")),
        subject(
            "subscribeRest.nats jobs.>",
            created,
            "jobs.created",
            Some("jobs.>"),
        ),
        subject(
            "subscribeRest.nats jobs.>",
            created_eu,
            "jobs.eu.created",
            Some("jobs.>"),
        ),
        subject(
            "subscribeRest.nats jobs.>",
            created_audit,
            "jobs.audit.created",
            Some("jobs.>"),
        ),
        subject(
            "onAudit",
            created_audit,
            "jobs.audit.created",
            Some("jobs.audit.>"),
        ),
        subject(
            "subscribeMiddle",
            created_eu,
            "jobs.eu.created",
            Some("jobs.*.created"),
        ),
        subject(
            "subscribeMiddle",
            created_audit,
            "jobs.audit.created",
            Some("jobs.*.created"),
        ),
        subject("subscribeMiddle", created, "jobs.created", None),
    ];
    expected.sort();
    assert_eq!(links, expected);
}

/// The BullMQ and Redis stream forms of the TS/JS pack (spec #20), as `(consumer, producer,
/// reason)`, every link `exact`: `Queue.add` and `Worker`, ioredis `xadd`/`xreadgroup` and
/// node-redis `xAdd`/`xReadGroup`, client wrappers included. A client of another package, a
/// `Set.add` and a name Meada cannot evaluate produce no link.
#[test]
fn typescript_and_javascript_bullmq_and_redis() {
    let repo = fixture_repo("conformance_jobs", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let mut links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .filter(|l| {
            let reason = l["reason"].as_str().unwrap();
            reason.starts_with("channel::bullmq::") || reason.starts_with("channel::redis::")
        })
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .collect();
    links.sort();

    let bull = "src/messages/bullmq.ts::";
    let redis = "src/messages/redis.ts::";
    let job = |consumer: &str, producer: &str, name: &str| {
        let (consumer, producer) = (format!("{bull}{consumer}"), format!("{bull}{producer}"));
        (
            consumer,
            producer,
            format!("channel::bullmq::{name}"),
            "exact",
        )
    };
    let stream = |consumer: &str, producer: &str, name: &str| {
        let (consumer, producer) = (format!("{redis}{consumer}"), format!("{redis}{producer}"));
        (
            consumer,
            producer,
            format!("channel::redis::{name}"),
            "exact",
        )
    };
    let mut expected = vec![
        job("runJob", "enqueueAll::jobs.add#0", "jobs"),
        // The queue is named by the binding, so a wrapper's `add` is the producer.
        job("runJob", "enqueue::jobs.add#0", "jobs"),
        job(
            "startMail.bullmq mail-eu",
            "enqueueAll::mail.add#0",
            "mail-eu",
        ),
        stream("readIoredis", "appendIoredis::ioredis.xadd#0", "orders"),
        stream("readMany", "appendIoredis::ioredis.xadd#0", "orders"),
        stream("readIoredis", "appendIoredis::named.xadd#0", "events.audit"),
        stream(
            "readNodeRedis",
            "appendNodeRedis::client.xAdd#0",
            "shipments",
        ),
        stream("readMany", "appendNodeRedis::other.xAdd#0", "returns"),
        stream("readNamed", "appendBilling::append#0", "billing"),
        stream("readBilling", "appendBilling::append#0", "billing"),
    ];
    expected.sort();
    assert_eq!(links, expected);
    // The two names Meada cannot evaluate stay as unresolved sides of their broker kind.
    let unresolved = |broker: &str| {
        (json["providers"].as_array().unwrap())
            .iter()
            .filter(|p| p["contract"] == format!("channel::{broker}"))
            .count()
    };
    assert_eq!((unresolved("bullmq"), unresolved("redis")), (1, 1));
}

/// The amqplib forms of the TS/JS pack (spec #20): `sendToQueue`, `consume`, `bindQueue` and
/// `publish` on a channel that traces back to `amqplib` (a confirm channel too), as
/// `(consumer, producer, reason, level)`.
#[test]
fn typescript_and_javascript_amqplib() {
    let repo = fixture_repo("conformance_amqplib", "conformance-ts-js", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();

    let links: Vec<(String, String, String, &str)> = (json["links"].as_array().unwrap())
        .iter()
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap(),
            )
        })
        .filter(|(_, _, reason, _)| reason.starts_with("channel::amqp::"))
        .collect();

    let queues = "src/messages/queues.ts::";
    let exchanges = "src/messages/exchanges.ts::";
    let link = |consumer: &str, producer: &str, queue: &str| {
        (
            consumer.to_string(),
            format!("{queues}{producer}"),
            format!("channel::amqp::{queue}"),
            "exact",
        )
    };
    // A publish to the exchange `events` reaches the queues whose binding matches its key.
    let published = |handler: &str, queue: &str, call: usize, binding: &str, level| {
        (
            format!("{exchanges}{handler}"),
            format!("{exchanges}announce::channel.publish#{call}"),
            format!("channel::amqp::{queue} binding events {binding}"),
            level,
        )
    };
    let mut expected = vec![
        link(
            &format!("{queues}work.amqp tasks"),
            "enqueue::channel.sendToQueue#0",
            "tasks",
        ),
        link(
            "src/messages/handlers.ts::onPayment",
            "enqueue::confirming.sendToQueue#0",
            "invoices",
        ),
        link(
            &format!("{queues}workReports"),
            "enqueueReports::queue#0",
            "reports",
        ),
        published("onAudit", "audit", 0, "order.created", "exact"),
        published("onAlert", "alerts", 0, "order.#", "inferred"),
        published("onAlert", "alerts", 1, "order.#", "inferred"),
        published("onShipment", "shipments", 1, "*.shipped", "inferred"),
        // The empty exchange is the queue `tasks`.
        (
            format!("{queues}work.amqp tasks"),
            format!("{exchanges}announce::channel.publish#2"),
            "channel::amqp::tasks".to_string(),
            "exact",
        ),
    ];
    expected.sort();
    assert_eq!(links, expected);
}

/// The channel sides of `file` in the Python fixture: the export, its producers as `(contract,
/// site)` and its consumers as `(contract, via, site)`, sorted, with repository-relative sites.
/// An unresolved side has the contract `channel::<broker>`.
fn python_channels(case: &str, file: &str) -> (Value, Vec<(String, String)>, Vec<Consumer>) {
    let dir = format!("messages/{file}::");
    let (json, producers) = python_providers(case, &dir);
    let consumers = consumers(&json, &dir);
    (json, producers, consumers)
}

/// A link of the export as `(consumer, producer, reason, level)`, ids relative to the
/// repository.
type ChannelLink = (String, String, String, String);

/// The links of `json` whose reason starts with `prefix`, sorted.
fn channel_links(json: &Value, prefix: &str) -> Vec<ChannelLink> {
    let rel = |id: &str| id.strip_prefix(&format!("{REPO}::")).unwrap().to_string();
    let mut links: Vec<ChannelLink> = (json["links"].as_array().unwrap())
        .iter()
        .filter(|l| l["reason"].as_str().unwrap().starts_with(prefix))
        .map(|l| {
            (
                rel(l["consumer"].as_str().unwrap()),
                rel(l["provider"].as_str().unwrap()),
                l["reason"].as_str().unwrap().to_string(),
                l["level"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    links.sort();
    links
}

/// A link as `channel_links` lists it.
fn link(consumer: String, producer: String, reason: &str, level: &str) -> ChannelLink {
    (consumer, producer, reason.into(), level.into())
}

/// The Kafka forms of the Python pack (spec #20, #73): confluent-kafka and kafka-python, as
/// the sides of each file and the links between them, all `exact`. A producer of another
/// package and `produce` on an object of the repository produce nothing; a topic from the
/// environment and a client wrapper reached only by a method's name are unresolved producers.
#[test]
fn python_kafka_clients_are_channel_producers_and_consumers() {
    let confluent = |rest: &str| format!("messages/confluent.py::{rest}");
    let (json, producers, consumers) = python_channels("python_kafka_confluent", "confluent.py");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::orders",
                &confluent("produce_all::producer.produce#0")
            ),
            (
                "channel::kafka::payments",
                &confluent("produce_all::producer.produce#1")
            ),
            (
                "channel::kafka::refunds.eu",
                &confluent("produce_all::other.produce#0")
            ),
            ("channel::kafka::refunds", &confluent("emit_refund::emit#0")),
            (
                "channel::kafka",
                &confluent("produce_unknown::producer.produce#0")
            ),
            (
                "channel::kafka",
                &confluent("Bus.relay::producer.produce#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::kafka::orders", "", &confluent("consume_orders")),
            ("channel::kafka::payments", "", &confluent("consume_orders")),
            ("channel::kafka::refunds", "", &confluent("consume_refunds")),
            (
                "channel::kafka::refunds.eu",
                "",
                &confluent("consume_refunds")
            ),
            // A topic that starts with `^` is a regular expression, so it names no topic.
            ("channel::kafka", "", &confluent("consume_pattern")),
        ])
    );

    let kafka = |rest: &str| format!("messages/kafka_python.py::{rest}");
    let (_, producers, consumers) = python_channels("python_kafka_python", "kafka_python.py");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::invoices",
                &kafka("send_invoice::producer.send#0")
            ),
            (
                "channel::kafka::retries",
                &kafka("send_retry::producer.send#0")
            ),
            (
                "channel::kafka::audits",
                &kafka("Relay.forward::self.producer.send#0")
            ),
        ])
    );
    // The constructor's topics and `subscribe(topics=)`; a constructor with none is no consumer.
    assert_eq!(
        consumers,
        triples(&[
            ("channel::kafka::invoices", "", &kafka("listen_invoices")),
            ("channel::kafka::audits", "", &kafka("listen_invoices")),
            ("channel::kafka::retries", "", &kafka("listen_retries")),
            // A constructor imported under another name.
            ("channel::kafka::alias", "", &kafka("listen_alias")),
        ])
    );

    let mut expected = vec![
        link(
            confluent("consume_orders"),
            confluent("produce_all::producer.produce#0"),
            "channel::kafka::orders",
            "exact",
        ),
        link(
            confluent("consume_orders"),
            confluent("produce_all::producer.produce#1"),
            "channel::kafka::payments",
            "exact",
        ),
        link(
            confluent("consume_refunds"),
            confluent("emit_refund::emit#0"),
            "channel::kafka::refunds",
            "exact",
        ),
        link(
            confluent("consume_refunds"),
            confluent("produce_all::other.produce#0"),
            "channel::kafka::refunds.eu",
            "exact",
        ),
        link(
            kafka("listen_invoices"),
            kafka("send_invoice::producer.send#0"),
            "channel::kafka::invoices",
            "exact",
        ),
        link(
            kafka("listen_invoices"),
            kafka("Relay.forward::self.producer.send#0"),
            "channel::kafka::audits",
            "exact",
        ),
        link(
            kafka("listen_retries"),
            kafka("send_retry::producer.send#0"),
            "channel::kafka::retries",
            "exact",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::kafka::"), expected);

    // A name rebound in its scope (a parameter, `with`, `for`, `except`, the constructor's own
    // name) is no client of the library.
    let (_, producers, consumers) = python_channels("python_kafka_shadowed", "shadowed.py");
    assert_eq!((producers, consumers), (vec![], vec![]));
}

/// The pika forms of the Python pack (spec #20, #73): `basic_publish`, `basic_consume` and
/// `queue_bind` on a channel that traces back to `pika.BlockingConnection`, as sides and links.
/// A publish to the default exchange is the queue its routing key names; one to `events` reaches
/// the queues bound to it, `inferred` for a wildcard pattern. A channel of another package
/// produces nothing.
#[test]
fn python_pika_channels_are_amqp_producers_consumers_and_bindings() {
    let rabbit = |rest: &str| format!("messages/pika_amqp.py::{rest}");
    let (json, producers, consumers) = python_channels("python_pika", "pika_amqp.py");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::amqp::tasks",
                &rabbit("enqueue::channel.basic_publish#0")
            ),
            (
                "channel::amqp::invoices",
                &rabbit("enqueue::channel.basic_publish#1")
            ),
            (
                "channel::amqp::reports",
                &rabbit("enqueue_reports::queue#0")
            ),
            (
                "channel::amqp",
                &rabbit("enqueue_unknown::channel.basic_publish#0")
            ),
            (
                "channel::amqp::mail",
                &rabbit("send_mail::sender.basic_publish#0")
            ),
            (
                "channel::amqp::reports",
                &rabbit("Worker.enqueue::self.channel.basic_publish#0")
            ),
            // Clients written as a chain of calls.
            (
                "channel::amqp::chained",
                &rabbit("send_chained::sender.basic_publish#0")
            ),
            (
                "channel::amqp::inline",
                &rabbit("send_chained::(call).basic_publish#0")
            ),
        ])
    );
    // A lambda is no definition: the function that consumes handles.
    assert_eq!(
        consumers,
        triples(&[
            ("channel::amqp::audit", "", &rabbit("on_audit")),
            ("channel::amqp::alerts", "", &rabbit("on_alert")),
            ("channel::amqp::shipments", "", &rabbit("listen")),
            ("channel::amqp::tasks", "", &rabbit("on_task")),
            ("channel::amqp::mail", "", &rabbit("on_mail")),
            ("channel::amqp::reports", "", &rabbit("Worker.on_report")),
        ])
    );

    let published = |queue: &str, handler: &str, call: usize, binding: &str, level: &str| {
        link(
            rabbit(handler),
            rabbit(&format!("announce::channel.basic_publish#{call}")),
            &format!("channel::amqp::{queue} binding events {binding}"),
            level,
        )
    };
    let direct = |handler: &str, producer: &str, queue: &str| {
        link(
            rabbit(handler),
            rabbit(producer),
            &format!("channel::amqp::{queue}"),
            "exact",
        )
    };
    let mut expected = vec![
        direct("on_task", "enqueue::channel.basic_publish#0", "tasks"),
        direct("Worker.on_report", "enqueue_reports::queue#0", "reports"),
        direct(
            "Worker.on_report",
            "Worker.enqueue::self.channel.basic_publish#0",
            "reports",
        ),
        direct("on_mail", "send_mail::sender.basic_publish#0", "mail"),
        published("audit", "on_audit", 0, "order.created", "exact"),
        published("alerts", "on_alert", 0, "order.#", "inferred"),
        published("alerts", "on_alert", 1, "order.#", "inferred"),
        published("shipments", "listen", 1, "*.shipped", "inferred"),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::amqp::"), expected);
}

/// The boto3 forms of the Python pack (spec #20, #73): `send_message`, `send_message_batch` and
/// `receive_message` on a client made for `"sqs"` (also through a `Session`), the queue being
/// the last segment of the URL. A client of another service or package produces nothing; a
/// URL read from the environment is an unresolved consumer.
#[test]
fn python_boto3_sqs_clients_are_channel_producers_and_consumers() {
    let sqs = |rest: &str| format!("messages/sqs_boto.py::{rest}");
    let (json, producers, consumers) = python_channels("python_sqs", "sqs_boto.py");
    assert_eq!(
        producers,
        pairs(&[
            ("channel::sqs::jobs", &sqs("send_job::sqs.send_message#0")),
            ("channel::sqs::mail", &sqs("send_job::sqs.send_message#1")),
            (
                "channel::sqs::jobs-dlq",
                &sqs("send_joined::sqs.send_message#0")
            ),
            (
                "channel::sqs::audit",
                &sqs("send_batch::sqs.send_message_batch#0")
            ),
            ("channel::sqs::billing", &sqs("enqueue_billing::enqueue#0")),
            (
                "channel::sqs::session",
                &sqs("send_session::queues.send_message#0")
            ),
            // Clients written as a chain of calls; the one made for `"s3"` is no producer.
            (
                "channel::sqs::chained",
                &sqs("send_chained::chained.send_message#0")
            ),
            (
                "channel::sqs::inline",
                &sqs("send_chained::(call).send_message#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::sqs::jobs", "", &sqs("receive_jobs")),
            ("channel::sqs::mail", "", &sqs("receive_mail")),
            ("channel::sqs::billing", "", &sqs("receive_billing")),
            ("channel::sqs::session", "", &sqs("receive_session")),
            ("channel::sqs", "", &sqs("receive_unknown")),
        ])
    );

    let queue = |handler: &str, producer: &str, name: &str| {
        link(
            sqs(handler),
            sqs(producer),
            &format!("channel::sqs::{name}"),
            "exact",
        )
    };
    let mut expected = vec![
        queue("receive_jobs", "send_job::sqs.send_message#0", "jobs"),
        queue("receive_mail", "send_job::sqs.send_message#1", "mail"),
        queue("receive_billing", "enqueue_billing::enqueue#0", "billing"),
        queue(
            "receive_session",
            "send_session::queues.send_message#0",
            "session",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::sqs::"), expected);
}

/// The nats-py forms of the Python pack (spec #20, #73): `publish` and `subscribe` on a
/// connection from `nats.connect` (awaited, returned by a function or passed by a caller) or a
/// `Client`, with the callback from `cb=`. A wildcard subject links at `inferred`, a connection
/// of another package and `publish` on any object produce nothing.
#[test]
fn python_nats_connections_are_channel_producers_and_consumers() {
    let nats = |rest: &str| format!("messages/nats_py.py::{rest}");
    let (json, producers, consumers) = python_channels("python_nats", "nats_py.py");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::nats::jobs.created",
                &nats("publish_all::nc.publish#0")
            ),
            (
                "channel::nats::jobs.eu.created",
                &nats("publish_all::nc.publish#1")
            ),
            (
                "channel::nats::jobs.audit.created",
                &nats("publish_all::other.publish#0")
            ),
            (
                "channel::nats::jobs.legacy",
                &nats("publish_legacy::nc.publish#0")
            ),
            (
                "channel::nats::refunds",
                &nats("publish_returned::nc.publish#0")
            ),
            (
                "channel::nats::jobs.announced",
                &nats("announce::nc.publish#0")
            ),
            ("channel::nats", &nats("publish_unknown::nc.publish#0")),
        ])
    );
    // A `cb` that is no definition: the function that subscribes handles.
    assert_eq!(
        consumers,
        triples(&[
            ("channel::nats::jobs.*", "", &nats("subscribe_one")),
            ("channel::nats::jobs.>", "", &nats("on_job")),
            ("channel::nats::jobs.audit.>", "", &nats("subscribe_rest")),
            (
                "channel::nats::jobs.*.created",
                "",
                &nats("subscribe_middle")
            ),
            ("channel::nats::jobs.created", "", &nats("subscribe_middle")),
            ("channel::nats::refunds", "", &nats("subscribe_middle")),
        ])
    );

    let subject = |handler: &str, producer: &str, subject: &str, pattern: Option<&str>| {
        let (handler, producer) = (nats(handler), nats(producer));
        match pattern {
            Some(pattern) => link(
                handler,
                producer,
                &format!("channel::nats::{subject} wildcard {pattern}"),
                "inferred",
            ),
            None => link(
                handler,
                producer,
                &format!("channel::nats::{subject}"),
                "exact",
            ),
        }
    };
    let created = "publish_all::nc.publish#0";
    let created_eu = "publish_all::nc.publish#1";
    let created_audit = "publish_all::other.publish#0";
    let legacy = "publish_legacy::nc.publish#0";
    let announced = "announce::nc.publish#0";
    let mut expected = vec![
        subject("subscribe_one", created, "jobs.created", Some("jobs.*")),
        subject("subscribe_one", legacy, "jobs.legacy", Some("jobs.*")),
        subject("subscribe_one", announced, "jobs.announced", Some("jobs.*")),
        subject("on_job", created, "jobs.created", Some("jobs.>")),
        subject("on_job", created_eu, "jobs.eu.created", Some("jobs.>")),
        subject(
            "on_job",
            created_audit,
            "jobs.audit.created",
            Some("jobs.>"),
        ),
        subject("on_job", legacy, "jobs.legacy", Some("jobs.>")),
        subject("on_job", announced, "jobs.announced", Some("jobs.>")),
        subject(
            "subscribe_rest",
            created_audit,
            "jobs.audit.created",
            Some("jobs.audit.>"),
        ),
        subject(
            "subscribe_middle",
            created_eu,
            "jobs.eu.created",
            Some("jobs.*.created"),
        ),
        subject(
            "subscribe_middle",
            created_audit,
            "jobs.audit.created",
            Some("jobs.*.created"),
        ),
        subject("subscribe_middle", created, "jobs.created", None),
        subject(
            "subscribe_middle",
            "publish_returned::nc.publish#0",
            "refunds",
            None,
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::nats::"), expected);
}

/// The redis-py stream forms of the Python pack (spec #20, #73): `xadd`, and the stream keys of
/// `xreadgroup` (a literal, a constant or an f-string). A read with no consumer group and a
/// client of another package produce nothing.
#[test]
fn python_redis_streams_are_channel_producers_and_consumers() {
    let redis = |rest: &str| format!("messages/redis_streams.py::{rest}");
    let (json, producers, consumers) = python_channels("python_redis", "redis_streams.py");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::redis::orders",
                &redis("append_all::client.xadd#0")
            ),
            (
                "channel::redis::shipments",
                &redis("append_all::client.xadd#1")
            ),
            (
                "channel::redis::shipments.eu",
                &redis("append_all::pool.xadd#0")
            ),
            (
                "channel::redis::returns",
                &redis("append_all::imported.xadd#0")
            ),
            (
                "channel::redis::events.audit",
                &redis("append_audit::append#0")
            ),
            ("channel::redis", &redis("append_unknown::client.xadd#0")),
            // The old class name, and `from_url` of `redis` and of `redis.asyncio`.
            (
                "channel::redis::legacy",
                &redis("append_more::legacy.xadd#0")
            ),
            (
                "channel::redis::by-url",
                &redis("append_more::by_url.xadd#0")
            ),
            (
                "channel::redis::async-by-url",
                &redis("append_more::async_by_url.xadd#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::redis::orders", "", &redis("read_orders")),
            ("channel::redis::orders", "", &redis("read_many")),
            ("channel::redis::shipments.eu", "", &redis("read_many")),
            ("channel::redis", "", &redis("read_unknown")),
        ])
    );

    let stream = |handler: &str, producer: &str, name: &str| {
        link(
            redis(handler),
            redis(producer),
            &format!("channel::redis::{name}"),
            "exact",
        )
    };
    let mut expected = vec![
        stream("read_orders", "append_all::client.xadd#0", "orders"),
        stream("read_many", "append_all::client.xadd#0", "orders"),
        stream("read_many", "append_all::pool.xadd#0", "shipments.eu"),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::redis::"), expected);
}

/// The channel sides of `file` in the Rust fixture's worker crate: the export, its producers as
/// `(contract, site)` and its consumers as `(contract, via, site)`, sorted, with
/// repository-relative sites. An unresolved side has the contract `channel::<broker>`.
fn rust_channels(case: &str, file: &str) -> (Value, Vec<(String, String)>, Vec<Consumer>) {
    let dir = format!("crates/worker/src/{file}::");
    let (json, producers) = providers(case, "conformance-rust", &dir);
    let consumers = consumers(&json, &dir);
    (json, producers, consumers)
}

/// The rdkafka forms of the Rust pack (spec #30): `send` of a `FutureRecord` or `BaseRecord`
/// on a producer, and `subscribe` on a consumer, both made by `ClientConfig`, whichever way the
/// client and the record reach the call. A channel's `send` and a transport of the repository
/// produce nothing; a topic from the environment is unresolved, and so is a regular expression.
#[test]
fn rust_rdkafka_clients_are_channel_producers_and_consumers() {
    let kafka = |rest: &str| format!("crates/worker/src/kafka.rs::{rest}");
    let (json, producers, consumers) = rust_channels("rust_kafka", "kafka.rs");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::orders",
                &kafka("produce_all::FutureRecord.to#0")
            ),
            (
                "channel::kafka::payments",
                &kafka("produce_all::FutureRecord.to#1")
            ),
            (
                "channel::kafka::refunds.eu",
                &kafka("produce_all::FutureRecord.to#2")
            ),
            (
                "channel::kafka::invoices",
                &kafka("produce_all::FutureRecord.to#3")
            ),
            (
                "channel::kafka::audits",
                &kafka("base_produce::BaseRecord.to#0")
            ),
            ("channel::kafka::retries", &kafka("emit_refund::emit#0")),
            (
                "channel::kafka",
                &kafka("produce_unknown::FutureRecord.to#0")
            ),
        ])
    );
    // A polling client: the function that subscribes handles.
    assert_eq!(
        consumers,
        triples(&[
            ("channel::kafka::orders", "", &kafka("consume_orders")),
            ("channel::kafka::payments", "", &kafka("consume_orders")),
            ("channel::kafka::refunds.eu", "", &kafka("consume_refunds")),
            ("channel::kafka::retries", "", &kafka("consume_refunds")),
            ("channel::kafka::invoices", "", &kafka("consume_refunds")),
            ("channel::kafka::audits", "", &kafka("consume_refunds")),
            ("channel::kafka", "", &kafka("consume_pattern")),
        ])
    );

    let topic = |handler: &str, producer: &str, name: &str| {
        link(
            kafka(handler),
            kafka(producer),
            &format!("channel::kafka::{name}"),
            "exact",
        )
    };
    let mut expected = vec![
        topic("consume_orders", "produce_all::FutureRecord.to#0", "orders"),
        topic(
            "consume_orders",
            "produce_all::FutureRecord.to#1",
            "payments",
        ),
        topic(
            "consume_refunds",
            "produce_all::FutureRecord.to#2",
            "refunds.eu",
        ),
        topic(
            "consume_refunds",
            "produce_all::FutureRecord.to#3",
            "invoices",
        ),
        topic("consume_refunds", "base_produce::BaseRecord.to#0", "audits"),
        topic("consume_refunds", "emit_refund::emit#0", "retries"),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::kafka::"), expected);

    // A parameter of the repository's own type, and a producer no caller passes, are no clients.
    let (_, producers, consumers) = rust_channels("rust_kafka_shadowed", "mq_shadowed.rs");
    assert_eq!((producers, consumers), (vec![], vec![]));
}

/// A builder chain is read to 64 calls (`CHAIN` in `channel/rust.rs`): a record or an SQS
/// request with more setters than that is no producer, and one with fewer is.
#[test]
fn rust_builder_chains_past_the_bound_are_not_read() {
    let repo = fixture_repo("rust_chain_bound", "conformance-rust", Some(ORIGIN));
    let setters = |call: &str, n: usize| call.repeat(n);
    let src = format!(
        r#"use aws_sdk_sqs::Client;
use rdkafka::config::ClientConfig;
use rdkafka::producer::{{FutureProducer, FutureRecord}};
use std::time::Duration;

pub async fn bounded() {{
    let client = Client::new(&aws_config::load_from_env().await);
    let producer: FutureProducer = ClientConfig::new().create().expect("producer");
    let wait = Duration::from_secs(0);
    let _ = producer.send(FutureRecord::to("short.record"){}, wait).await;
    let _ = producer.send(FutureRecord::to("long.record"){}, wait).await;
    let _ = client.send_message(){}.queue_url("https://sqs.eu-west-1.amazonaws.com/1/short").send().await;
    let _ = client.send_message(){}.queue_url("https://sqs.eu-west-1.amazonaws.com/1/long").send().await;
}}
"#,
        setters(".payload(\"p\")", 10),
        setters(".payload(\"p\")", 100),
        setters(".message_body(\"b\")", 10),
        setters(".message_body(\"b\")", 100),
    );
    let worker = repo.join("crates/worker/src");
    std::fs::write(worker.join("bounded.rs"), src).unwrap();
    let lib = std::fs::read_to_string(worker.join("lib.rs")).unwrap();
    std::fs::write(worker.join("lib.rs"), format!("{lib}\nmod bounded;\n")).unwrap();

    let export = Engine::open(&repo).unwrap().export().unwrap();
    let json: Value = serde_json::from_str(&export).unwrap();
    let mut names: Vec<&str> = (json["providers"].as_array().unwrap().iter())
        .filter(|p| {
            p["site"]
                .as_str()
                .unwrap()
                .contains("worker/src/bounded.rs")
        })
        .map(|p| p["contract"].as_str().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["channel::kafka::short.record", "channel::sqs::short"]
    );
}

/// The lapin forms of the Rust pack (spec #30): `basic_publish`, `basic_consume` and
/// `queue_bind` on a channel that traces back to `Connection::connect`, as sides and links. A
/// publish to the default exchange is the queue its key names; one to `events` reaches the
/// queues bound to it, `inferred` for a wildcard pattern. A binding that cannot be evaluated is
/// dropped, and an exchange that cannot makes an unresolved producer.
#[test]
fn rust_lapin_channels_are_amqp_producers_consumers_and_bindings() {
    let rabbit = |rest: &str| format!("crates/worker/src/rabbit.rs::{rest}");
    let (json, producers, consumers) = rust_channels("rust_lapin", "rabbit.rs");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::amqp::jobs",
                &rabbit("publish_all::channel.basic_publish#0")
            ),
            (
                "channel::amqp",
                &rabbit("publish_unknown::channel.basic_publish#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::amqp::jobs", "", &rabbit("work_jobs")),
            ("channel::amqp::audit", "", &rabbit("audit")),
            ("channel::amqp", "", &rabbit("audit")),
        ])
    );

    let mut expected = vec![
        link(
            rabbit("work_jobs"),
            rabbit("publish_all::channel.basic_publish#0"),
            "channel::amqp::jobs",
            "exact",
        ),
        link(
            rabbit("audit"),
            rabbit("publish_all::channel.basic_publish#1"),
            "channel::amqp::audit binding events order.#",
            "inferred",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::amqp::"), expected);
}

/// The aws-sdk-sqs forms of the Rust pack (spec #30): the `queue_url` of a `send_message`,
/// `send_message_batch` or `receive_message` builder on a client from `Client::new`, the queue
/// being the last segment of the URL. Another call on the queue and a builder of the repository
/// produce nothing; a URL read from the environment is an unresolved producer.
#[test]
fn rust_aws_sdk_sqs_clients_are_channel_producers_and_consumers() {
    let sqs = |rest: &str| format!("crates/worker/src/queues.rs::{rest}");
    let (json, producers, consumers) = rust_channels("rust_sqs", "queues.rs");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::sqs::jobs",
                &sqs("send_all::(call_expression).queue_url#0")
            ),
            (
                "channel::sqs::billing",
                &sqs("send_all::(call_expression).queue_url#1")
            ),
            (
                "channel::sqs::mail",
                &sqs("send_all::(call_expression).queue_url#2")
            ),
            ("channel::sqs::audit", &sqs("enqueue_audit::enqueue#0")),
            (
                "channel::sqs::prefixed",
                &sqs("send_prefixed::(call_expression).queue_url#0")
            ),
            (
                "channel::sqs",
                &sqs("send_unknown::(call_expression).queue_url#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::sqs::jobs", "", &sqs("receive_jobs")),
            ("channel::sqs::billing", "", &sqs("receive_many")),
        ])
    );

    let mut expected = vec![
        link(
            sqs("receive_jobs"),
            sqs("send_all::(call_expression).queue_url#0"),
            "channel::sqs::jobs",
            "exact",
        ),
        link(
            sqs("receive_many"),
            sqs("send_all::(call_expression).queue_url#1"),
            "channel::sqs::billing",
            "exact",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::sqs::"), expected);
}

/// The async-nats forms of the Rust pack (spec #30): `publish`, `subscribe` and
/// `queue_subscribe` on a client from `async_nats::connect` or `ConnectOptions`, whichever way
/// the crate is named. A wildcard subject links at `inferred`; a bus of the repository produces
/// nothing and a subject from the environment is an unresolved producer.
#[test]
fn rust_async_nats_clients_are_channel_producers_and_consumers() {
    let nats = |rest: &str| format!("crates/worker/src/subjects.rs::{rest}");
    let (json, producers, consumers) = rust_channels("rust_nats", "subjects.rs");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::nats::orders.created",
                &nats("publish_all::client.publish#0")
            ),
            (
                "channel::nats::orders.shipped",
                &nats("publish_all::client.publish#1")
            ),
            (
                "channel::nats::audit.login",
                &nats("publish_all::client.publish#2")
            ),
            (
                "channel::nats::mail.sent",
                &nats("publish_with_options::client.publish#0")
            ),
            (
                "channel::nats",
                &nats("publish_with_options::client.publish#1")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::nats::orders.created", "", &nats("listen_orders")),
            ("channel::nats::orders.*", "", &nats("listen_orders")),
            ("channel::nats::audit.>", "", &nats("listen_queue")),
            ("channel::nats::mail.sent", "", &nats("listen_queue")),
        ])
    );

    let exact = |handler: &str, producer: &str, subject: &str| {
        link(
            nats(handler),
            nats(producer),
            &format!("channel::nats::{subject}"),
            "exact",
        )
    };
    let wild = |handler: &str, producer: &str, subject: &str, pattern: &str| {
        link(
            nats(handler),
            nats(producer),
            &format!("channel::nats::{subject} wildcard {pattern}"),
            "inferred",
        )
    };
    let mut expected = vec![
        exact(
            "listen_orders",
            "publish_all::client.publish#0",
            "orders.created",
        ),
        wild(
            "listen_orders",
            "publish_all::client.publish#0",
            "orders.created",
            "orders.*",
        ),
        wild(
            "listen_orders",
            "publish_all::client.publish#1",
            "orders.shipped",
            "orders.*",
        ),
        wild(
            "listen_queue",
            "publish_all::client.publish#2",
            "audit.login",
            "audit.>",
        ),
        exact(
            "listen_queue",
            "publish_with_options::client.publish#0",
            "mail.sent",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::nats::"), expected);
}

/// The redis forms of the Rust pack (spec #30): `xadd` and the keys of `xread_options` whose
/// options carry a group, on a connection that traces back to `Client::open`. A read without a
/// group, and an `xadd` of the repository's own type, produce nothing; a key from the
/// environment is an unresolved side.
#[test]
fn rust_redis_streams_are_channel_producers_and_consumers() {
    let redis = |rest: &str| format!("crates/worker/src/streams.rs::{rest}");
    let (json, producers, consumers) = rust_channels("rust_redis", "streams.rs");
    assert_eq!(
        producers,
        pairs(&[
            ("channel::redis::orders", &redis("append_all::con.xadd#0")),
            (
                "channel::redis::shipments",
                &redis("append_all::con.xadd#1")
            ),
            (
                "channel::redis::events.audit",
                &redis("append_all::con.xadd#2")
            ),
            ("channel::redis", &redis("append_all::con.xadd#3")),
            ("channel::redis::kept", &redis("hidden_names::con.xadd#2")),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::redis::orders", "", &redis("read_group")),
            ("channel::redis::shipments", "", &redis("read_group")),
            ("channel::redis::events.audit", "", &redis("read_inline")),
            ("channel::redis", "", &redis("read_unknown")),
        ])
    );

    let stream = |handler: &str, producer: &str, name: &str| {
        link(
            redis(handler),
            redis(producer),
            &format!("channel::redis::{name}"),
            "exact",
        )
    };
    let mut expected = vec![
        stream("read_group", "append_all::con.xadd#0", "orders"),
        stream("read_group", "append_all::con.xadd#1", "shipments"),
        stream("read_inline", "append_all::con.xadd#2", "events.audit"),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::redis::"), expected);
}

/// The channel sides of `file` in the Go fixture's `messages` package: the export, its producers
/// as `(contract, site)` and its consumers as `(contract, via, site)`, sorted, with
/// repository-relative sites. An unresolved side has the contract `channel::<broker>`.
fn go_channels(case: &str, file: &str) -> (Value, Vec<(String, String)>, Vec<Consumer>) {
    let dir = format!("messages/{file}::");
    let (json, producers) = providers(case, "conformance-go", &dir);
    let consumers = consumers(&json, &dir);
    (json, producers, consumers)
}

/// The Kafka forms of the Go pack (spec #25, #82): segmentio/kafka-go, confluent-kafka-go and
/// sarama, as the sides of each file and the links between them. A topic the writer names
/// stands for its messages; a writer a function made, an environment read and a regular
/// expression leave the side unresolved.
#[test]
fn go_kafka_clients_are_channel_producers_and_consumers() {
    let at = |file: &str, rest: &str| format!("messages/{file}.go::{rest}");
    let segmentio = |rest: &str| at("segmentio", rest);
    let (json, producers, consumers) = go_channels("go_kafka_segmentio", "segmentio.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::orders",
                &segmentio("ProduceAll::orders.WriteMessages#0")
            ),
            (
                "channel::kafka::payments.eu",
                &segmentio("ProduceAll::payments.WriteMessages#0")
            ),
            (
                "channel::kafka::audits",
                &segmentio("ProduceMessages::bare.WriteMessages#0")
            ),
            (
                "channel::kafka::audits.eu",
                &segmentio("ProduceMessages::bare.WriteMessages#0")
            ),
            (
                "channel::kafka::retries",
                &segmentio("ProduceMessages::bare.WriteMessages#1")
            ),
            (
                "channel::kafka::invoices",
                &segmentio("ProduceMessages::bare.WriteMessages#1")
            ),
            (
                "channel::kafka::refunds",
                &segmentio("ProduceMade::w.WriteMessages#0")
            ),
            (
                "channel::kafka::refund.requested",
                &segmentio("EmitRefund::emit#0")
            ),
            (
                "channel::kafka",
                &segmentio("ProduceUnknown::w.WriteMessages#0")
            ),
            // The repository's `NewWriter` is not kafka-go's, and a wrapper nobody calls keeps
            // its parameter.
            (
                "channel::kafka",
                &segmentio("ProduceLookalike::w.WriteMessages#0")
            ),
            (
                "channel::kafka",
                &segmentio("Publisher.Send::p.w.WriteMessages#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::kafka::orders", "", &segmentio("ConsumeOrders")),
            (
                "channel::kafka::refunds.eu",
                "",
                &segmentio("ConsumeRefunds")
            ),
            ("channel::kafka::retries", "", &segmentio("ConsumeRefunds")),
            ("channel::kafka::invoices", "", &segmentio("ConsumeRefunds")),
            ("channel::kafka", "", &segmentio("ConsumeUnknown")),
        ])
    );

    let confluent = |rest: &str| at("confluent", rest);
    let (_, producers, consumers) = go_channels("go_kafka_confluent", "confluent.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::orders",
                &confluent("ConfluentProduce::p.Produce#0")
            ),
            (
                "channel::kafka::orders",
                &confluent("ConfluentProduce::p.Produce#1")
            ),
            (
                "channel::kafka",
                &confluent("ConfluentProduce::p.Produce#2")
            ),
            // The pointer is written through.
            ("channel::kafka", &confluent("ConfluentDeref::p.Produce#0")),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            (
                "channel::kafka::payments",
                "",
                &confluent("ConfluentConsume")
            ),
            (
                "channel::kafka::refunds.eu",
                "",
                &confluent("ConfluentConsume")
            ),
            (
                "channel::kafka::retries",
                "",
                &confluent("ConfluentConsume")
            ),
            ("channel::kafka", "", &confluent("ConfluentPattern")),
        ])
    );

    let sarama = |rest: &str| at("sarama", rest);
    let (_, producers, consumers) = go_channels("go_kafka_sarama", "sarama.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::kafka::orders",
                &sarama("SaramaProduce::producer.SendMessage#0")
            ),
            (
                "channel::kafka::payments",
                &sarama("SaramaProduce::producer.SendMessage#1")
            ),
            ("channel::kafka", &sarama("SaramaSend::p.SendMessage#0")),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::kafka::refunds.eu", "", &sarama("SaramaPartition")),
            (
                "channel::kafka::orders",
                "",
                &sarama("orderHandler.ConsumeClaim")
            ),
            (
                "channel::kafka::payments",
                "",
                &sarama("orderHandler.ConsumeClaim")
            ),
            (
                "channel::kafka::invoices",
                "",
                &sarama("orderHandler.ConsumeClaim")
            ),
        ])
    );

    // The handler of a group is its `ConsumeClaim`; a reader's is the function that built it.
    let links = channel_links(&json, "channel::kafka::");
    for (consumer, producer, reason) in [
        (
            sarama("orderHandler.ConsumeClaim"),
            segmentio("ProduceAll::orders.WriteMessages#0"),
            "channel::kafka::orders",
        ),
        (
            segmentio("ConsumeOrders"),
            confluent("ConfluentProduce::p.Produce#1"),
            "channel::kafka::orders",
        ),
        (
            confluent("ConfluentConsume"),
            segmentio("ProduceMessages::bare.WriteMessages#1"),
            "channel::kafka::retries",
        ),
    ] {
        assert!(
            links.contains(&link(consumer, producer, reason, "exact")),
            "{reason}"
        );
    }
}

/// The RabbitMQ forms of the Go pack: the default exchange reaches the queue its key names,
/// exactly; another exchange is a binding match; an exchange from a parameter leaves the
/// producer unresolved.
#[test]
fn go_amqp_channels_are_producers_consumers_and_bindings() {
    let rabbit = |rest: &str| format!("messages/rabbit.go::{rest}");
    let (json, producers, consumers) = go_channels("go_amqp", "rabbit.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::amqp::jobs",
                &rabbit("PublishAll::ch.PublishWithContext#0")
            ),
            ("channel::amqp", &rabbit("PublishUnknown::ch.Publish#0")),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::amqp::jobs", "", &rabbit("WorkJobs")),
            ("channel::amqp::audit", "", &rabbit("Audit")),
        ])
    );
    let reason = "channel::amqp::audit binding events order.#";
    let mut expected = vec![
        link(
            rabbit("WorkJobs"),
            rabbit("PublishAll::ch.PublishWithContext#0"),
            "channel::amqp::jobs",
            "exact",
        ),
        link(
            rabbit("Audit"),
            rabbit("PublishAll::ch.PublishWithContext#1"),
            reason,
            "inferred",
        ),
        link(
            rabbit("Audit"),
            rabbit("PublishAll::ch.Publish#0"),
            reason,
            "inferred",
        ),
    ];
    expected.sort();
    assert_eq!(channel_links(&json, "channel::amqp::"), expected);
}

/// The queue clients of the Go pack: SQS queue names are the last segment of the queue URL,
/// folded through `aws.String`, an environment read leaves the producer unresolved; NATS
/// callbacks are the handlers, wildcards link as inferred; only a Redis read in a consumer
/// group is a consumer.
#[test]
fn go_sqs_nats_and_redis_clients_are_channel_producers_and_consumers() {
    let queues = |rest: &str| format!("messages/queues.go::{rest}");
    let (json, producers, consumers) = go_channels("go_sqs", "queues.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::sqs::tasks",
                &queues("SqsProduce::client.SendMessage#0")
            ),
            ("channel::sqs", &queues("SqsProduce::client.SendMessage#1")),
            (
                "channel::sqs::invoices",
                &queues("SqsProduce::client.SendMessageBatch#0")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::sqs::tasks", "", &queues("SqsConsume")),
            ("channel::sqs::reports", "", &queues("SqsConsume")),
        ])
    );
    assert_eq!(
        channel_links(&json, "channel::sqs::"),
        vec![link(
            queues("SqsConsume"),
            queues("SqsProduce::client.SendMessage#0"),
            "channel::sqs::tasks",
            "exact"
        )]
    );

    let subjects = |rest: &str| format!("messages/subjects.go::{rest}");
    let (json, producers, consumers) = go_channels("go_nats", "subjects.go");
    assert_eq!(
        producers,
        pairs(&[
            (
                "channel::nats::orders.created",
                &subjects("NatsPublish::nc.Publish#0")
            ),
            (
                "channel::nats::orders.updated",
                &subjects("NatsPublish::nc.Publish#1")
            ),
        ])
    );
    assert_eq!(
        consumers,
        triples(&[
            (
                "channel::nats::orders.created",
                "",
                &subjects("NatsSubscribe.nats orders.created")
            ),
            ("channel::nats::orders.*", "", &subjects("onOrder")),
            ("channel::nats::audit.>", "", &subjects("auditor.onAudit")),
        ])
    );
    let links = channel_links(&json, "channel::nats::");
    assert!(links.contains(&link(
        subjects("onOrder"),
        subjects("NatsPublish::nc.Publish#1"),
        "channel::nats::orders.updated wildcard orders.*",
        "inferred"
    )));

    let streams = |rest: &str| format!("messages/streams.go::{rest}");
    let (json, producers, consumers) = go_channels("go_redis", "streams.go");
    assert_eq!(
        producers,
        pairs(&[(
            "channel::redis::orders",
            &streams("RedisProduce::rdb.XAdd#0")
        )])
    );
    assert_eq!(
        consumers,
        triples(&[
            ("channel::redis::orders", "", &streams("RedisConsume")),
            ("channel::redis::payments", "", &streams("RedisConsume")),
        ])
    );
    assert_eq!(
        channel_links(&json, "channel::redis::"),
        vec![link(
            streams("RedisConsume"),
            streams("RedisProduce::rdb.XAdd#0"),
            "channel::redis::orders",
            "exact"
        )]
    );
}

/// A receiver that does not trace back to the library's constructor makes no side, whatever
/// its methods are called.
#[test]
fn go_channel_methods_of_the_repositorys_own_types_are_no_sides() {
    let (_, producers, consumers) = go_channels("go_shadowed", "shadowed.go");
    assert_eq!((producers, consumers), (vec![], vec![]));
}

/// A field of the writer written after its literal (`w.Topic = ..`) leaves the topic unknown.
#[test]
fn go_kafka_writer_topic_written_after_the_literal_is_unresolved() {
    let (_, producers) = providers("go_kafka_written", "go-channels-written", "writer.go::");
    assert_eq!(
        producers,
        pairs(&[("channel::kafka", "writer.go::Produce::w.WriteMessages#0")])
    );
}
