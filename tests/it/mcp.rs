use crate::common;

use common::fixture_repo;
use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParams, ClientCapabilities, ClientConfig, ErrorData, Implementation},
    service::{RequestContext, RoleClient},
    transport::{ConfigureCommandExt, TokioChildProcess},
};

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#192)"
)]
use rmcp::model::{ListRootsResult, Root};

/// `meada mcp` in a fixture repository: the tools it lists stay within 1,000 tokens (one per
/// four bytes of description and parameter schema, ADR 0008), and `trace` answers.
#[tokio::test]
async fn serves_three_tools_over_stdio_within_the_description_budget() {
    let repo = fixture_repo(
        "mcp_smoke",
        "ts-basic",
        Some("https://example.com/acme/web.git"),
    );
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&repo);
        }),
    )
    .unwrap();
    let client = ().serve(transport).await.unwrap();

    let tools = client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| &*t.name).collect();
    names.sort();
    assert_eq!(names, ["context", "search", "trace"]);
    let bytes: usize = tools
        .iter()
        .map(|t| {
            t.description.as_deref().unwrap_or("").len()
                + serde_json::to_string(&*t.input_schema).unwrap().len()
        })
        .sum();
    assert!(
        bytes.div_ceil(4) <= 1_000,
        "tool list costs {} tokens",
        bytes.div_ceil(4)
    );

    let answer = client
        .call_tool(
            CallToolRequestParams::new("trace").with_arguments(
                serde_json::json!({"from": "sum", "direction": "down"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let text = answer.content.first().and_then(|c| c.as_text()).unwrap();
    assert_eq!(
        text.text,
        "example.com/acme/web src/math.ts:5 sum\n  example.com/acme/web src/math.ts:1 add\n\
         chain: exact · 0 hops · 1 repos · 32 tokens\n"
    );
    client.cancel().await.unwrap();
}

/// The client's first `file://` root wins over the server's cwd (#192): a client that answers
/// `roots/list` with another repository, under a directory with a space in its name, gets
/// answers about that repository.
#[tokio::test]
async fn answers_about_the_clients_first_root_not_its_cwd() {
    let cwd = fixture_repo(
        "mcp_roots_cwd",
        "ts-basic",
        Some("https://example.com/acme/cwd.git"),
    );
    let root = fixture_repo(
        "mcp roots root",
        "ts-basic",
        Some("https://example.com/acme/root.git"),
    );
    let transport = TokioChildProcess::new(
        tokio::process::Command::new(env!("CARGO_BIN_EXE_meada")).configure(|c| {
            c.arg("mcp").current_dir(&cwd);
        }),
    )
    .unwrap();
    let uri = format!("file://{}", root.display().to_string().replace(' ', "%20"));
    let client = Roots(uri).serve(transport).await.unwrap();

    let answer = client
        .call_tool(
            CallToolRequestParams::new("trace").with_arguments(
                serde_json::json!({"from": "sum", "direction": "down"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let text = answer.content.first().and_then(|c| c.as_text()).unwrap();
    assert!(
        text.text
            .starts_with("example.com/acme/root src/math.ts:5 sum\n"),
        "{}",
        text.text
    );
    client.cancel().await.unwrap();
}

/// A client that declares the roots capability and answers `roots/list` with one `file://` root.
struct Roots(String);

#[expect(
    deprecated,
    reason = "roots are deprecated by SEP-2577; the server still follows them (#192)"
)]
impl ClientHandler for Roots {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_roots().build(),
            Implementation::from_build_env(),
        )
    }

    async fn list_roots(
        &self,
        _: RequestContext<RoleClient>,
    ) -> Result<ListRootsResult, ErrorData> {
        Ok(ListRootsResult::new(vec![Root::new(self.0.clone())]))
    }
}
