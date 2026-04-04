use anyhow::{bail, Context, Result};
use reqwest::Client;
use std::time::Duration;

/// Retry an HTTP GET with exponential backoff.
///
/// - 404 → bail immediately with "{service}: not found"
/// - non-429 4xx → bail immediately
/// - 429 / 5xx / network error → retry up to `max_retries` times
pub async fn retry_get(
    client: &Client,
    url: &str,
    service: &str,
    max_retries: u32,
    backoff_base_ms: u64,
) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..=max_retries {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(
                backoff_base_ms * (1 << (attempt - 1)),
            ))
            .await;
        }
        match client.get(url).send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                if (200..300).contains(&status) {
                    return resp
                        .text()
                        .await
                        .with_context(|| format!("{service}: reading response body"));
                }
                if status == 404 {
                    bail!("{service}: not found");
                }
                if status != 429 && (400..500).contains(&status) {
                    bail!("{service}: http {status}");
                }
                last_err = format!("http {status}");
            }
            Err(e) => {
                last_err = e.to_string();
            }
        }
    }
    bail!("{service}: failed after {max_retries} retries: {last_err}")
}
