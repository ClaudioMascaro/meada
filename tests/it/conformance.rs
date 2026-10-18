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
        ],
    );
    // The call inside `println!` is not indexed, and neither is a file no `mod` reaches.
    let repo = fixture_repo("conformance_rust_absent", "conformance-rust", Some(ORIGIN));
    let export = Engine::open(&repo).unwrap().export().unwrap();
    assert!(!export.contains(&format!("{MAIN}::main::compute#")));
    assert!(!export.contains("orphan.rs"));
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
        "crates/billing/src/billing.rs Invoice crates/billing/src/invoice.rs Invoice",
        "crates/billing/src/billing.rs invoice crates/billing/src/invoice.rs -",
        "crates/billing/src/billing.rs settle crates/billing/src/billing.rs settle",
        "crates/billing/src/billing.rs total crates/billing/src/billing.rs total",
        "crates/billing/src/invoice.rs Invoice crates/billing/src/invoice.rs Invoice",
        "crates/billing/src/invoice.rs tax crates/billing/src/invoice.rs tax",
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

/// The leaf routes of a ladder of `levels` URLconfs, each including the next twice.
fn django_ladder(case: &str, levels: usize) -> Vec<String> {
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
            "from django.urls import include, path\n\nurlpatterns = [\n{}{}]\n",
            include("a"),
            include("b")
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
    let mut routes = django_ladder("python_django_diamond", 2);
    routes.sort();
    assert_eq!(
        routes,
        ["/a/a/x", "/a/b/x", "/b/a/x", "/b/b/x"].map(|p| format!("http::ANY::{p}"))
    );
    // A ladder of 24 levels has 2^24 paths: the budget stops the walk well before them.
    let routes = django_ladder("python_django_ladder", 24);
    assert!(!routes.is_empty());
    assert!(routes.len() < 1 << 20, "{}", routes.len());
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
