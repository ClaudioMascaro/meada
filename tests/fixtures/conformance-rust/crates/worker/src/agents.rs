use ureq::Agent;

pub fn module_functions() -> Result<(), ureq::Error> {
    ureq::get("https://api.example.com/u/get").call()?;
    ureq::post("https://api.example.com/u/post").send_string("")?;
    ureq::put("https://api.example.com/u/put").call()?;
    ureq::patch("https://api.example.com/u/patch").call()?;
    ureq::delete("https://api.example.com/u/delete").call()?;
    ureq::head("https://api.example.com/u/head").call()?;
    ureq::request("OPTIONS", "https://api.example.com/u/request").call()?;
    Ok(())
}

pub fn agents() -> Result<(), ureq::Error> {
    let agent = ureq::agent();
    agent.get("https://api.example.com/agent/get").call()?;
    agent
        .request("DELETE", "https://api.example.com/agent/request")
        .call()?;
    let built = ureq::AgentBuilder::new().build();
    built.post("https://api.example.com/agent/built").send_string("")?;
    let named = Agent::new();
    named.put("https://api.example.com/agent/new").send_string("")?;
    Ok(())
}
