use anyhow::Result;
use clap::Parser;
use env_logger;
use log::*;
use serde_json::Value;
use tungstenite::Message;
use std::time::Instant;
use std::time::Duration;
use tokio::time;
use tokio_tungstenite::connect_async;
use core::num::NonZeroU32;
use futures::SinkExt;
use futures::StreamExt;
use rand::seq::SliceRandom;
use anyhow::anyhow;
use rand::Rng;
use hex;

// Arguments needed to run this test
#[derive(Parser, Default, Debug)]
#[command(author, version, about, long_about)]
struct Arguments {
    #[arg(short = 'c', long = "clients", default_value_t = 1)]
    clients: usize,
    #[arg(short, long, default_value_t = 10000)]
    requests: usize,
    #[arg(short = 'u', long, default_value = "ws://localhost:8080")]
    relay_url: String,
}

// Results
#[derive(Debug, PartialEq, Clone)]
struct BenchResult {
    start: Instant, // when this test was started
    duration: Duration, // duration required to run entire test.
    success_count: usize, // how many EOSEs were received.
    failure_count: usize, // how many NOTICEs were received.
    timings: Vec<TestResult>, // individual timings for each client response
}

// Individual Test Result
#[derive(Debug, PartialEq, Clone)]
struct TestResult {
    response: QueryResult,
    start: Instant,
    duration: Duration,
}

#[derive(Debug, PartialEq, Clone)]
enum QueryResult {
    Eose,
    Notice,
}


#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Arguments::parse();
    let test_start = Instant::now();
    info!("Args: {:?}", args);
    // responses will be sent to this MPSC channel:
    let (res_tx, mut res_rx) = tokio::sync::mpsc::unbounded_channel::<TestResult>();
    // start clients
    let url = args.relay_url;
    for _ in 0..args.clients {
	let rtx = res_tx.clone();
	let u = url.clone();
	info!("spawning client...");
	tokio::spawn(async move {
	    client(u, rtx).await.ok();
	});
    }
    let mut responses = 0;
    // read responses into a benchresult
    loop {
	tokio::select! {
	    tr = res_rx.recv() => {
		if let Some(r) = tr {
		    info!("got a result: {:?}", r);
		    responses += 1;
		    if responses >= args.requests {
			break;
		    }
		} else {
		    // channel is closed, end
		    break;
		}
	    }
	}
    }
    info!("Completed test in {:?}", test_start.elapsed());
}

// generate a random 64-char hex string
fn random_id() -> String {
    let bytes : [u8; 1] = rand::random();
    hex::encode(bytes)
}

async fn client(relay_url: String, res_tx: tokio::sync::mpsc::UnboundedSender<TestResult>) -> Result<()>{
    // connect to relay
    info!("about to connect...: {:?}", relay_url);
    let (mut client,_) = connect_async(relay_url).await?;
    info!("connected to relay");
    let mut prev_event_id_hex = random_id();
    loop {
	let mut rand_event_id_hex = random_id();
	// generate a random event ID to query on.
	while prev_event_id_hex == rand_event_id_hex {
	    rand_event_id_hex = random_id();
	}
	prev_event_id_hex = rand_event_id_hex.clone();
	let msg = format!(r##"
["REQ", "random-event-eose-concurr", {{"limit": 1000, "ids": ["{}"]}}]
"##, rand_event_id_hex);
	let start_inst = Instant::now();
	client.send(Message::Text(msg.into())).await.ok();
	let mut events_recvd = 0;
	loop {
	    let res = client.next().await;
	    // check for successful response
	    if let Some(Ok(Message::Text(x))) = res {
		if x.starts_with("[\"EOSE\",") {
		    info!("req cleared after {} events", events_recvd);
		    events_recvd = 0;
		    break;
		} else if x.starts_with("[\"NOTICE\",") {
		    break;
		} else {
		    events_recvd += 1;
		    // this was just a normal event, keep reading.
		}
	    } else {
		info!("there was an error getting a response: {:?}", res);
		break;
	    }
	}
	res_tx.send(TestResult {response: QueryResult::Eose, start: start_inst, duration: start_inst.elapsed()})?;
    }
}
