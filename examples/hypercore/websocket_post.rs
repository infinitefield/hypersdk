//! Send info requests over the WebSocket instead of over HTTP.
//!
//! The socket accepts anything `/info` and `/exchange` accept (except `explorer` requests),
//! which saves a round of TCP and TLS setup when you already hold an open connection.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example websocket-post
//! ```
//!
//! # What it does
//!
//! 1. Opens a WebSocket to mainnet
//! 2. Posts two info requests with distinct ids
//! 3. Matches each reply back to its id and prints a summary
//!
//! Replies arrive as [`Incoming::Post`] on the normal event stream, so `id` is what ties a
//! response to its request. Posts are not replayed across reconnects: if the connection drops
//! before the server answers, no reply for that id arrives.

use std::collections::HashMap;

use futures::StreamExt;
use hypersdk::hypercore::{
    self as hypercore,
    types::{Incoming, PostRequest, PostResponsePayload},
    ws::Event,
};
use serde_json::json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ = simple_logger::init_with_level(log::Level::Info);

    let mut ws = hypercore::mainnet().websocket();

    // Distinct ids so the replies can be told apart.
    let mut pending: HashMap<u64, &str> = HashMap::new();
    pending.insert(1, "meta");
    pending.insert(2, "l2Book(BTC)");

    ws.post(1, PostRequest::Info(json!({ "type": "meta" })));
    ws.post(
        2,
        PostRequest::Info(json!({ "type": "l2Book", "coin": "BTC" })),
    );

    while let Some(event) = ws.next().await {
        let Event::Message(Incoming::Post(post)) = event else {
            continue;
        };

        let label = pending.remove(&post.id).unwrap_or("<unknown id>");
        match post.response {
            PostResponsePayload::Info(value) => {
                // The result sits under `data`; `type` echoes the info request type.
                let summary = serde_json::to_string(&value["data"])?;
                let head: String = summary.chars().take(90).collect();
                println!("[{}] {label}: {head}...", post.id);
            }
            PostResponsePayload::Action(response) => {
                println!("[{}] {label}: {response:?}", post.id);
            }
            PostResponsePayload::Error(err) => {
                println!("[{}] {label} failed: {err}", post.id);
            }
        }

        if pending.is_empty() {
            break;
        }
    }

    Ok(())
}
