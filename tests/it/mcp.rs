use crate::common;

use common::fixture_repo;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{ConfigureCommandExt, TokioChildProcess},
};

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
