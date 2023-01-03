use anyhow::Result;
use clap::Parser;
use env_logger;
use governor::{Jitter, Quota, RateLimiter};
use log::*;
use serde_json::Value;
use tungstenite::Message;
use std::fs;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::Duration;
use std::time::Instant;
use tokio::time;
use tokio_tungstenite::connect_async;
use core::num::NonZeroU32;
use futures::SinkExt;
use futures::StreamExt;
use rand::seq::SliceRandom;
use anyhow::anyhow;

#[derive(Parser, Default, Debug)]
#[command(author, version, about, long_about)]
struct Arguments {
    #[arg(short = 'c', long = "clients", default_value_t = 10)]
    client_count: usize,
    #[arg(short, long, default_value_t = 2)]
    reqs_per_min: usize,
    #[arg(short, long, default_value_t = 3)]
    subs_per_client: usize,
    #[arg(short = 'd', long)]
    req_directory: String,
    #[arg(short = 'u', long, default_value = "ws://localhost:8080")]
    relay_url: String,
    #[arg(long, default_value_t = 100)]
    client_read_per_sec: usize,
}

type ReqArc = Arc<Mutex<Vec<TestReq>>>;

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Arguments::parse();
    println!("Args: {:?}", args);
    // Get the set of Reqs on disk
    let requests = get_all_reqs(&args.req_directory).unwrap();
    // wrap it in an Arc for sending to clients
    let requests = Arc::new(Mutex::new(requests));
    for _ in 0..args.client_count {
        let url = args.relay_url.clone();
	let client_reqs = requests.clone();
        tokio::spawn(async move {
            client(&url, client_reqs, args.reqs_per_min, args.subs_per_client).await.ok();
        });
    }
    time::sleep(Duration::from_secs(100000)).await;
}

/// Create a request governor
fn per_min_governor(
    per_min: usize,
) -> RateLimiter<
    governor::state::NotKeyed,
    governor::state::InMemoryState,
    governor::clock::DefaultClock,
    governor::middleware::NoOpMiddleware,
> {
    let quota_time = NonZeroU32::new(per_min.try_into().unwrap()).unwrap();
    let quota = Quota::per_minute(quota_time).allow_burst(NonZeroU32::new(1u32).unwrap());
    RateLimiter::direct(quota)
}

// get a random REQ
fn get_req(reqs: ReqArc) -> TestReq {
    // get a read lock
    let rs = reqs.lock().unwrap();
    // select a random
    rs.choose(&mut rand::thread_rng()).unwrap().clone()
}

/// Manage client subscriptions
struct SubManager {
    subscriptions: Arc<RwLock<Vec<Subscription>>>,
    capacity: usize,
}

impl SubManager {
    /// Get a new name for a sub, overwriting one if we are at capacity
    fn get_new_sub_name() -> String {
	"new-sub".into()
    }
    /// Return how many subscriptions are open
    fn subs_in_use(&self) -> usize {
	self.subscriptions.read().unwrap().len()
    }
    /// Mark a subscription as closed
    fn close_sub(name: &str) {
    }
    /// Record that a subscription has no more stored events
    fn register_eose(name: &str) {
    }
    /// Record that an event was just observed for this sub.
    fn register_event(name: &str) {
    }
}

/// A client subscription
struct Subscription {
    name: String, // name registered with relay
    created: Instant, // when this sub was created
    last_recv: Instant, // when the last event was received for this sub
    eose_recv: bool, // whether an EOSE notice was received for this sub
}

/// A nostr client task
async fn client(relay_url: &str, reqs: ReqArc, req_per_min: usize, max_subs: usize) -> Result<()> {
    // clients each have a pool of subscriptions they can use.  Each
    // sub has a name, state (open/close), and last-response
    // timestamp.
    let req_rate_limiter = per_min_governor(req_per_min);
    let jitter = Jitter::up_to(Duration::from_millis(1600));
    // Connect
    let (mut client,_) = connect_async(relay_url).await?;
    loop {
        tokio::select! {
            _ = req_rate_limiter.until_ready_with_jitter(jitter) => {
		info!("sending a message");
		let req_template = get_req(reqs.clone());
		let named_req = req_template.with_named_sub(&req_template.name)?;
		info!("named_req: {:?}", named_req);
		let req = named_req.to_string(); //.ok_or(anyhow!("could not make string"))?;
		info!("this req: {:?}", req);
		client.send(Message::Text(req.into())).await.ok();

	    },
		_ws_next = client.next() => {
            }
        }
    }
}

/// A REQ for testing
#[derive(Debug, PartialEq, Clone)]
struct TestReq {
    name: String,
    value: serde_json::Value,
}

impl TestReq {
    /// Produce a REQ value with named sub
    fn with_named_sub(&self, name: &str) -> Result<serde_json::Value> {
	let mut req = vec!["REQ".into(), name.into()];
	match &self.value{
	    Value::Array(filters) => {
		for f in filters {
		    if let Value::Object(_) = f {
			req.push(f.clone());
		    }
		}
	    },
	    _ => {
		return Err(anyhow!("Reqs must be an array of filters"));
	    }
	}
	// create a new
	Ok(Value::Array(req))
    }
}

/// Get all the REQ files in a directory.
fn get_all_reqs(req_dir: &str) -> Result<Vec<TestReq>> {
    let mut reqs = vec![];
    for entry in fs::read_dir(req_dir)? {
        let entry = entry?;
        let path = entry.path();
        let metadata = fs::metadata(&path)?;
        if metadata.is_file() {
            // read contents
            let contents = fs::read_to_string(&path)?;
            let name = path.file_name().map(|n| n.to_str()).flatten().unwrap_or("unknown").into();
            let value: Value = serde_json::from_str(&contents)?;
            reqs.push(TestReq { name, value });
        }
    }
    Ok(reqs)
}
