use aws_sdk_sqs::Client;

const BILLING_URL: &str = "https://sqs.eu-west-1.amazonaws.com/123456789012/billing";

/// A client a function makes: the callers' `client` traces back to the constructor through it.
async fn sqs_client() -> Client {
    Client::new(&aws_config::load_from_env().await)
}

pub async fn send_all() -> Result<(), aws_sdk_sqs::Error> {
    let config = aws_config::load_from_env().await;
    let client = Client::new(&config);
    client
        .send_message()
        .queue_url("https://sqs.eu-west-1.amazonaws.com/123456789012/jobs")
        .message_body("j")
        .send()
        .await?;
    // The setter may come before the queue, and the URL be a constant.
    client
        .send_message()
        .message_body("b")
        .queue_url(BILLING_URL)
        .send()
        .await?;
    client
        .send_message_batch()
        .queue_url("https://sqs.eu-west-1.amazonaws.com/123456789012/mail")
        .send()
        .await?;
    Ok(())
}

/// The client and the queue are parameters: each caller passes its own.
async fn enqueue(client: &Client, queue: &str) -> Result<(), aws_sdk_sqs::Error> {
    client
        .send_message()
        .queue_url(format!("https://sqs.eu-west-1.amazonaws.com/123456789012/{queue}"))
        .message_body("e")
        .send()
        .await?;
    Ok(())
}

pub async fn enqueue_audit() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    enqueue(&client, "audit").await
}

/// A prefix read from the environment before a literal tail: the queue is the last segment.
pub async fn send_prefixed() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    client
        .send_message()
        .queue_url(format!("{}/prefixed", std::env::var("QUEUE_BASE").unwrap()))
        .message_body("p")
        .send()
        .await?;
    Ok(())
}

pub async fn send_unknown() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    client
        .send_message()
        .queue_url(std::env::var("DEAD_LETTER_URL").unwrap())
        .message_body("u")
        .send()
        .await?;
    Ok(())
}

pub async fn receive_jobs() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    let _ = client
        .receive_message()
        .queue_url("https://sqs.eu-west-1.amazonaws.com/123456789012/jobs")
        .wait_time_seconds(20)
        .send()
        .await?;
    Ok(())
}

pub async fn receive_many() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    let _ = client.receive_message().queue_url(BILLING_URL).send().await?;
    Ok(())
}

/// Another call on the queue is neither side.
pub async fn other_calls() -> Result<(), aws_sdk_sqs::Error> {
    let client = sqs_client().await;
    client
        .delete_message()
        .queue_url("https://sqs.eu-west-1.amazonaws.com/123456789012/never")
        .receipt_handle("r")
        .send()
        .await?;
    Ok(())
}

pub struct Mailer;

impl Mailer {
    pub fn send_message(&self) -> Mailer {
        Mailer
    }

    pub fn queue_url(&self, _url: &str) -> Mailer {
        Mailer
    }
}

pub fn not_sqs() {
    Mailer.send_message().queue_url("https://example.com/never.local");
}
