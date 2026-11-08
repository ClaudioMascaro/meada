//! Config values (#639, ADR 0025): every scalar of the YAML and JSON files the walk visits, and
//! of the CI configuration under the hidden CI paths, found by `search` with the kind `value`.

use std::fs;

use meada::engine::{self, Engine};

use crate::common::{self, assert_golden, fixture_repo};

const ORIGIN: Option<&str> = Some("https://example.com/acme/app.git");

/// HCL JSON permits arrays around label maps and block bodies. The format's sensitive
/// markers hide defaults and tfvars in those forms, while ordinary values stay searchable.
#[test]
fn terraform_json_arrays_keep_marked_values_private() {
    let repo = fixture_repo(
        "config_json_secret_arrays",
        "terraform-json-secrets",
        ORIGIN,
    );
    let engine = Engine::open(&repo).unwrap();
    for secret in [
        "json-array-default-canary",
        "json-array-tfvars-canary",
        "json-array-ssm-canary",
    ] {
        assert_eq!(
            engine.search(secret, Some("value"), None).unwrap(),
            format!("no matches for `{secret}`\n")
        );
        assert!(!engine.export().unwrap().contains(secret));
    }
    let values = engine.search("default", Some("value"), None).unwrap();
    assert!(
        values.contains("variable[0][\"db-password\"][0].default = redacted"),
        "{values}"
    );
    for public in ["json-array-public", "json-array-ssm-public"] {
        let answer = engine.search(public, Some("value"), None).unwrap();
        assert!(answer.contains(&format!("= \"{public}\"")), "{answer}");
    }
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    for secret in [
        "json-array-default-canary",
        "json-array-tfvars-canary",
        "json-array-ssm-canary",
    ] {
        assert!(
            !index.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "{secret} in the stored index"
        );
    }
}

/// ADR 0025's SecureString marker hides the resource's value in both Terraform syntaxes,
/// even when the marker follows it. Ordinary String parameters remain searchable.
#[test]
fn secure_string_parameters_keep_their_values_out_of_storage_and_search() {
    let repo = fixture_repo("config_values_ssm", "terraform-ssm-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let values = engine
        .search("resource.aws_ssm_parameter", Some("value"), None)
        .unwrap();
    for file in ["main.tf", "main.tf.json"] {
        for name in ["before", "after"] {
            assert!(
                values.lines().any(|line| line.contains(&format!("{file}:"))
                    && line.ends_with(&format!(
                        "resource.aws_ssm_parameter.{name}.value = redacted"
                    ))),
                "{file}: {values}"
            );
        }
    }
    for public in ["ssm-hcl-public-value", "ssm-json-public-value"] {
        let answer = engine.search(public, Some("value"), None).unwrap();
        assert!(answer.contains(&format!("= \"{public}\"")), "{answer}");
    }
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    for secret in [
        "ssm-hcl-before-canary",
        "ssm-hcl-after-canary",
        "ssm-json-before-canary",
        "ssm-json-after-canary",
    ] {
        assert_eq!(
            engine.search(secret, Some("value"), None).unwrap(),
            format!("no matches for `{secret}`\n")
        );
        assert!(
            !index.windows(secret.len()).any(|w| w == secret.as_bytes()),
            "{secret} in the stored index"
        );
    }
}

/// The `search` answers of `queries` with the kind `value`, in order, a blank line between two.
fn searched(engine: &Engine, queries: &[&str]) -> String {
    (queries.iter())
        .map(|q| engine.search(q, Some("value"), None).unwrap())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fixture's Helm values carry #638's labels, its multi-document Kubernetes file a Secret,
/// `.github/workflows/ci.yml` a workflow; the editor's hidden `.vscode/` and the
/// `.meadaignore`d `secrets/` stay out, the lockfile and the Helm template that does not parse
/// are skipped. A query matches values and key paths, exact first, then prefix, then substring.
#[test]
fn search_with_the_kind_value_finds_config_values_by_value_or_key_path() {
    let repo = fixture_repo("config_values_search", "config-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "team-b",
        "support",
        "ghcr.io/acme/app",
        "password",
        "app.kubernetes.io",
        "description",
        "tags",
        "7357",
        "kept-out-by-meadaignore",
        "lockfileversion",
        "replicas",
        "flags",
    ];
    assert_golden("config-values.search.txt", &searched(&engine, &queries));
}

/// `meada status` counts the config files read and skipped (the lockfile, the Helm template
/// that does not parse) and the values redacted.
#[test]
fn status_counts_the_config_files_read_and_skipped_and_the_values_redacted() {
    let repo = fixture_repo("config_values_status", "config-values", ORIGIN);
    Engine::open(&repo).unwrap().export().unwrap();
    assert_golden("config-values.status.txt", &engine::status(&repo).unwrap());
}

/// With no kind, values answer only when no symbol and no contract matches: `health` finds the
/// function alone though `HealthCheckFailing` is a value, and `team-b` finds the values.
#[test]
fn the_default_search_falls_back_to_values_only_when_nothing_else_matches() {
    let repo = fixture_repo("config_values_default", "config-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let health = engine.search("health", None, None).unwrap();
    assert_eq!(
        health,
        engine.search("health", Some("symbol"), None).unwrap()
    );
    assert!(health.contains("healthCheck"), "{health}");
    assert!(!health.contains("HealthCheckFailing"), "{health}");
    assert!(
        (engine
            .search("HealthCheckFailing", Some("value"), None)
            .unwrap())
        .contains("alert"),
        "the value is indexed"
    );
    let team = engine.search("team-b", None, None).unwrap();
    assert_eq!(team, engine.search("team-b", Some("value"), None).unwrap());
    assert!(team.contains("chart/values-prd.yaml:6"), "{team}");
    let nothing = engine.search("no-such-value", None, None).unwrap();
    assert_eq!(nothing, "no matches for `no-such-value`\n");
}

/// Scalars stay out of `meada export` except names that form image contracts (#662), and a
/// Secret's values stay out of the index itself: its file
/// holds neither the encoded `data` nor the plain `stringData`.
#[test]
fn values_stay_out_of_the_export_and_a_secret_out_of_the_index() {
    let repo = fixture_repo("config_values_export", "config-values", ORIGIN);
    let export = Engine::open(&repo).unwrap().export().unwrap();
    for value in ["team-b", "HealthCheckFailing", "config/settings.json"] {
        assert!(!export.contains(value), "{value} in the export");
    }
    let written: serde_json::Value = serde_json::from_str(&export).unwrap();
    assert_eq!(
        written["providers"],
        serde_json::json!([
            {"contract": "image::ghcr.io/acme/app", "site": "example.com/acme/app", "at": ".github/workflows/ci.yml#jobs.image.steps[0].with.tags"},
            {"contract": "package::npm::app", "site": "example.com/acme/app::package.json"}
        ])
    );
    assert_eq!(
        written["consumers"],
        serde_json::json!([
            {"contract": "image::ghcr.io/acme/app", "site": "example.com/acme/app::chart/values-prd.yaml::image.repository", "via": "image: chart/values-prd.yaml#image.repository (Helm value)", "root": "chart"},
            {"contract": "image::ghcr.io/acme/app", "site": "example.com/acme/app::deploy/app.yaml::Deployment.apps/shop/app", "via": null, "root": "deploy"}
        ])
    );
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    let found = |needle: &[u8]| index.windows(needle.len()).any(|w| w == needle);
    assert!(found(b"team-b"), "the index holds the values");
    for secret in [&b"c2VjcmV0LXBhc3N3b3Jk"[..], b"app-admin"] {
        assert!(!found(secret), "{}", String::from_utf8_lossy(secret));
    }
}

/// Terraform and OpenTofu files (#640): `.tf`, `.tofu`, `.tf.json` and `.tfvars` files are
/// read, an attribute's key path running over its block's type and labels; a `.tofu` file shadows
/// the `.tf` file of its name; a file that does not parse is counted and skipped; a hidden
/// `.terraform/` stays out.
#[test]
fn search_with_the_kind_value_finds_terraform_values_by_block_path() {
    let repo = fixture_repo("config_values_terraform", "terraform-values", ORIGIN);
    let engine = Engine::open(&repo).unwrap();
    let queries = [
        "orders-queue",
        "payments-oncall",
        "payments-critical",
        "resource.opsgenie_team_routing_rule",
        "billing",
        "order-events",
        "legacy-queue-name",
        "tofu-queue-name",
        "alone-queue-name",
        "leaked-hidden-queue",
        "db_password",
        "default-db-secret",
        "root-tfvars-secret",
        "envs-tfvars-secret",
        "sibling-tfvars-secret",
        "under-broken-secret",
        "us-east-1",
        "cost-center",
        "redrive_policy",
        "arn:aws:sqs",
        "environment",
    ];
    assert_golden("terraform-values.search.txt", &searched(&engine, &queries));
}

/// `meada status` counts the Terraform files like the others: the unreadable `broken.tf` is
/// skipped, the `.tf` file a `.tofu` one shadows is not counted, the secrets are redacted.
#[test]
fn status_counts_the_terraform_files_read_and_skipped_and_the_values_redacted() {
    let repo = fixture_repo("config_values_terraform_status", "terraform-values", ORIGIN);
    Engine::open(&repo).unwrap().export().unwrap();
    assert_golden(
        "terraform-values.status.txt",
        &engine::status(&repo).unwrap(),
    );
}

/// No secret of a sensitive variable reaches the index, whether its default or a value any
/// `.tfvars` file gives it, while the plain values are there.
#[test]
fn a_sensitive_variable_and_the_tfvars_values_it_gets_stay_out_of_the_index() {
    let repo = fixture_repo("config_values_terraform_secret", "terraform-values", ORIGIN);
    Engine::open(&repo).unwrap().export().unwrap();
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    let found = |needle: &str| (index.windows(needle.len())).any(|w| w == needle.as_bytes());
    assert!(
        found("payments-oncall") && found("eu-west-1"),
        "the index holds the values"
    );
    for secret in [
        "default-db-secret",
        "root-tfvars-secret",
        "envs-tfvars-secret",
        "sibling-tfvars-secret",
        "under-broken-secret",
    ] {
        assert!(!found(secret), "{secret}");
    }
}

/// A `.tfvars` file is redacted for the sensitive variables of the Terraform files of its
/// directory and above: marking a variable sensitive, or no longer, in a file that is not the
/// `.tfvars` one redacts or shows its values at the next query, and so does removing the
/// `.tofu` file that shadowed a `.tf` file.
#[test]
fn a_tfvars_file_follows_the_sensitive_variables_of_the_files_around_it() {
    let repo = fixture_repo("config_values_terraform_fresh", "terraform-values", ORIGIN);
    common::age(&repo);
    let engine = Engine::open(&repo).unwrap();
    let value = |q: &str| engine.search(q, Some("value"), None).unwrap();
    let password = |line: &str| {
        let found = value("db_password");
        assert!(found.contains(line), "{line}\n{found}");
    };
    password("terraform.tfvars:2 db_password = redacted");
    password("envs/prd.tfvars:2 db_password = redacted");

    let main = repo.join("main.tf");
    let text = fs::read_to_string(&main).unwrap();
    fs::write(&main, text.replace("sensitive = true", "sensitive = false")).unwrap();
    common::age(&repo);
    password("terraform.tfvars:2 db_password = \"root-tfvars-secret\"");
    password("envs/prd.tfvars:2 db_password = \"envs-tfvars-secret\"");

    // Terraform converts the string "true" to the boolean.
    fs::write(
        &main,
        text.replace("sensitive = true", "sensitive = \"true\""),
    )
    .unwrap();
    common::age(&repo);
    password("terraform.tfvars:2 db_password = redacted");

    fs::write(&main, text).unwrap();
    common::age(&repo);
    password("terraform.tfvars:2 db_password = redacted");

    fs::remove_file(&main).unwrap();
    common::age(&repo);
    password("terraform.tfvars:2 db_password = \"root-tfvars-secret\"");

    assert!(value("tofu-queue-name").contains("shadow/queue.tofu:2"));
    assert_eq!(
        value("legacy-queue-name"),
        "no matches for `legacy-queue-name`\n"
    );
    fs::remove_file(repo.join("shadow/queue.tofu")).unwrap();
    common::age(&repo);
    assert!(value("legacy-queue-name").contains("shadow/queue.tf:2"));
    assert_eq!(
        value("tofu-queue-name"),
        "no matches for `tofu-queue-name`\n"
    );
}

/// A Terraform file under a hidden CI path is read like any other (#640): its `sensitive`
/// variable redacts the value a var file anywhere gives it, as ADR 0025 asks of "any Terraform
/// file of the repository" (#709, judging #639's second deviation: a filter of the hidden CI files
/// to YAML and JSON lost that redaction).
#[test]
fn a_sensitive_variable_under_a_hidden_ci_path_redacts_a_var_file_elsewhere() {
    let repo = fixture_repo("config_values_hidden_ci_sensitive", "config-values", ORIGIN);
    fs::create_dir_all(repo.join(".github/infra")).unwrap();
    fs::create_dir_all(repo.join("infra")).unwrap();
    fs::write(
        repo.join(".github/infra/variables.tf"),
        "variable \"gh_token\" {\n  sensitive = true\n}\n",
    )
    .unwrap();
    fs::write(
        repo.join("infra/terraform.tfvars"),
        "gh_token = \"hidden-ci-declared-secret\"\n",
    )
    .unwrap();
    let engine = Engine::open(&repo).unwrap();
    let found = engine
        .search("hidden-ci-declared-secret", Some("value"), None)
        .unwrap();
    assert_eq!(found, "no matches for `hidden-ci-declared-secret`\n");
    let redacted = engine.search("gh_token", Some("value"), None).unwrap();
    assert!(
        redacted.contains("infra/terraform.tfvars:1 gh_token = redacted"),
        "{redacted}"
    );
}

/// Terraform merges an override file's blocks into those of the other files of its directory: a
/// variable marked sensitive in one file keeps the default another file gives it out of the
/// index, an override file or a primary one, in HCL and in JSON, and so does an override file
/// that does not parse. Marking it so no longer, in the only file edited, shows the default at
/// the next query, and so does removing the override file.
#[test]
fn a_default_another_file_of_its_directory_marks_sensitive_stays_out_of_the_index() {
    let repo = fixture_repo("config_values_override_sensitive", "config-values", ORIGIN);
    let big = format!(
        "variable \"y\" {{\n  sensitive = true\n}}\n# {}\n",
        "x".repeat(1 << 20)
    );
    for (path, text) in [
        (
            "infra/main.tf",
            "variable \"db\" {\n  default = \"override-marked-secret\"\n}\n",
        ),
        (
            "infra/override.tf",
            "variable \"db\" {\n  sensitive = true\n}\n",
        ),
        (
            "infra/variables.tf",
            "variable \"token\" {\n  sensitive = true\n}\nvariable \"region\" {\n  default = \"eu-west-1\"\n}\n",
        ),
        (
            "infra/a_override.tf",
            "variable \"token\" {\n  default = \"override-given-secret\"\n}\n",
        ),
        (
            "json/main.tf.json",
            r#"{"variable": {"db": {"default": "json-override-secret"}}}"#,
        ),
        (
            "json/override.tf.json",
            r#"{"variable": {"db": {"sensitive": true}}}"#,
        ),
        (
            "broken/main.tf",
            "variable \"x\" {\n  default = \"broken-override-secret\"\n}\n",
        ),
        ("broken/x_override.tf", "variable \"x\" {\n"),
        (
            "big/main.tf",
            "variable \"y\" {\n  default = \"big-override-secret\"\n}\n",
        ),
        ("big/big_override.tf", &big),
    ] {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    // Static evaluation never puts one of them in a name either.
    for (dir, var) in [
        ("infra", "db"),
        ("infra", "token"),
        ("json", "db"),
        ("broken", "x"),
        ("big", "y"),
    ] {
        let queue =
            format!("resource \"aws_sqs_queue\" \"{var}\" {{ name = \"q-${{var.{var}}}\" }}\n");
        fs::write(repo.join(format!("{dir}/queue_{var}.tf")), queue).unwrap();
    }
    common::age(&repo);
    let engine = Engine::open(&repo).unwrap();
    let value = |q: &str| engine.search(q, Some("value"), None).unwrap();
    let shows = |q: &str, line: &str| {
        let found = value(q);
        assert!(found.contains(line), "{line}\n{found}");
    };
    shows(
        "variable.db.default",
        "infra/main.tf:2 variable.db.default = redacted",
    );
    shows(
        "variable.db.default",
        "json/main.tf.json:1 variable.db.default = redacted",
    );
    shows(
        "variable.token.default",
        "infra/a_override.tf:2 variable.token.default = redacted",
    );
    shows(
        "variable.x.default",
        "broken/main.tf:2 variable.x.default = redacted",
    );
    shows(
        "variable.y.default",
        "big/main.tf:2 variable.y.default = redacted",
    );
    let iac = engine.search("aws_sqs_queue", Some("iac"), None).unwrap();
    assert_eq!(iac.matches("pattern \"q-*\"").count(), 5, "{iac}");
    shows(
        "variable.region.default",
        "infra/variables.tf:5 variable.region.default = \"eu-west-1\"",
    );
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    let found = |needle: &str| (index.windows(needle.len())).any(|w| w == needle.as_bytes());
    for secret in [
        "override-marked-secret",
        "override-given-secret",
        "json-override-secret",
        "broken-override-secret",
        "big-override-secret",
    ] {
        assert!(!found(secret), "{secret}");
    }

    let text = "variable \"db\" {\n  sensitive = false\n}\n";
    fs::write(repo.join("infra/override.tf"), text).unwrap();
    fs::remove_file(repo.join("json/override.tf.json")).unwrap();
    common::age(&repo);
    shows(
        "variable.db.default",
        "infra/main.tf:2 variable.db.default = \"override-marked-secret\"",
    );
    shows(
        "variable.db.default",
        "json/main.tf.json:1 variable.db.default = \"json-override-secret\"",
    );
    shows(
        "variable.token.default",
        "infra/a_override.tf:2 variable.token.default = redacted",
    );
}

/// Terraform ignores `.tofu` files and reads the `.tf` file one shadows, which Meada does not
/// read: beside an override file it may mark any variable of its directory sensitive, so no
/// default there is stored or resolves into a name, and a var file in that directory keeps every
/// value redacted; elsewhere it changes nothing. An override file whose JSON root is an array of
/// bodies marks a variable as an object does. Removing the shadowed file shows the default again
/// at the next query.
#[test]
fn a_shadowed_terraform_file_and_a_json_root_array_keep_defaults_out_of_the_index() {
    let repo = fixture_repo("config_values_shadowed_sensitive", "config-values", ORIGIN);
    let default = |secret: &str| format!("variable \"db\" {{\n  default = \"{secret}\"\n}}\n");
    let marked = "variable \"db\" {\n  sensitive = true\n}\n";
    for (path, text) in [
        ("array/main.tf", default("array-marked-secret")),
        (
            "array/override.tf.json",
            r#"[{"variable": {"db": {"sensitive": true}}}]"#.into(),
        ),
        ("shadow/main.tf", default("shadow-marked-secret")),
        ("shadow/zz_override.tf", marked.into()),
        (
            "shadow/zz_override.tofu",
            "variable \"db\" {\n  description = \"x\"\n}\n".into(),
        ),
        ("broken/main.tf", default("broken-shadow-secret")),
        ("broken/override.tf", "locals {}\n".into()),
        ("broken/x.tf", format!("{marked}resource \"x\" {{\n")),
        ("broken/x.tofu", "variable \"other\" {}\n".into()),
        (
            "dual/main.tf",
            "variable \"dual_token\" {\n  sensitive = true\n}\n".into(),
        ),
        ("dual/main.tofu", "variable \"dual_token\" {}\n".into()),
        ("dual/override.tf", "locals {}\n".into()),
        (
            "dual/terraform.tfvars",
            "dual_token = \"dual-tfvars-secret\"\n".into(),
        ),
        ("twin/main.tf", "variable \"region\" {}\n".into()),
        ("twin/main.tofu", "variable \"region\" {}\n".into()),
        ("twin/terraform.tfvars", "region = \"eu-west-1\"\n".into()),
    ] {
        let path = repo.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
    for dir in ["array", "shadow", "broken"] {
        let queue = "resource \"aws_sqs_queue\" \"db\" { name = \"q-${var.db}\" }\n";
        fs::write(repo.join(format!("{dir}/queue.tf")), queue).unwrap();
    }
    common::age(&repo);
    let engine = Engine::open(&repo).unwrap();
    let shows = |q: &str, line: &str| {
        let found = engine.search(q, Some("value"), None).unwrap();
        assert!(found.contains(line), "{line}\n{found}");
    };
    for dir in ["array", "shadow", "broken"] {
        let line = format!("{dir}/main.tf:2 variable.db.default = redacted");
        shows("variable.db.default", &line);
    }
    shows(
        "dual_token",
        "dual/terraform.tfvars:1 dual_token = redacted",
    );
    shows("region", "twin/terraform.tfvars:1 region = \"eu-west-1\"");
    let iac = engine.search("aws_sqs_queue", Some("iac"), None).unwrap();
    assert_eq!(iac.matches("pattern \"q-*\"").count(), 3, "{iac}");
    let index = fs::read(repo.join(".meada/index.db")).unwrap();
    let found = |needle: &str| (index.windows(needle.len())).any(|w| w == needle.as_bytes());
    for secret in [
        "array-marked-secret",
        "shadow-marked-secret",
        "broken-shadow-secret",
        "dual-tfvars-secret",
    ] {
        assert!(!found(secret), "{secret}");
    }

    fs::remove_file(repo.join("shadow/zz_override.tf")).unwrap();
    common::age(&repo);
    shows(
        "variable.db.default",
        "shadow/main.tf:2 variable.db.default = \"shadow-marked-secret\"",
    );
    let iac = engine.search("aws_sqs_queue", Some("iac"), None).unwrap();
    assert!(iac.contains("name \"q-shadow-marked-secret\""), "{iac}");
}
