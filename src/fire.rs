use anyhow::Result;
use clap::{App, Arg, value_t};
use tokio::time::{sleep, Duration};
use firebase_rs::*;
use webrtc::api::media_engine::MediaEngine;
use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::{Registry};
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::{math_rand_alpha};
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Answer {
  answer: String
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::new("fire-channel")
        .version("0.4.1")
        .author("Artur Gwoździowski <gwozdyk@gmail.com>")
        .about("an example of creation p2p data-channel using firebase as signaling")
        .arg(
            Arg::new("IDENT")
                .help("some kind of callsign to identifying peer and data-channel name")
                .long("ident")
                .short('i')
                .takes_value(true)
                .default_value(&"robocik")
        )
        .get_matches();
        
    let identify = value_t!(app, "IDENT", String).unwrap();
    println!("[DEVICE]: {}", identify);
    sleep(Duration::from_millis(500)).await;
    println!("[STATE]: initialize...");
    sleep(Duration::from_secs(1)).await;
    let firebase = Firebase::new("https://rust-signal-default-rtdb.europe-west1.firebasedatabase.app")
        .unwrap().at("messages").at(&identify).at("offer");
    let offer_encoded = firebase.get::<String>().await;
    println!("[OFFER]: OK");

    let mut media = MediaEngine::default();
    media.register_default_codecs()?;
    let mut registry = Registry::new();
    registry = register_default_interceptors(registry, &mut media)?;

    let api = APIBuilder::new()
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .build();

    let config = RTCConfiguration {
        ice_servers: vec![RTCIceServer {
            urls: vec!["stun:stun.l.google.com:19302".to_owned()],
            ..Default::default()
        }],
        ..Default::default()
    };

    let peer_connection = Arc::new(api.new_peer_connection(config).await?);
    let (done_tx, mut done_rx) = tokio::sync::mpsc::channel::<()>(1);

    _ = Box::pin(async move {
        println!("reconnect timer start");
        loop {
            let reconnect_time = tokio::time::sleep(Duration::from_secs(5));
            tokio::pin!(reconnect_time);
            tokio::select! {
                _ = reconnect_time.as_mut() => {
                    println!("reconnect time!");
                }
            };
            ()
        }
    }).await;

    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("[STATE]: {}", s);
            if s == RTCPeerConnectionState::Failed {
                let _ = done_tx.try_send(());
            }
            Box::pin(async {})
        }))
        .await;

    peer_connection
        .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            Box::pin(async move {
                let d2 = Arc::clone(&d);
                d.on_open(Box::new(move || {
                    Box::pin(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(5));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() => {
                                    let message = math_rand_alpha(15);
                                    println!("<- '{}'", message);
                                    result = d2.send_text(message).await.map_err(Into::into);
                                }
                            };
                        }
                    })
                })).await;

                d.on_message(Box::new(move |msg: DataChannelMessage| {
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    println!("-> '{}'", msg_str);
                    Box::pin(async {})
                })).await;
            })
        }))
        .await;

    let desc_data = signal::decode(&offer_encoded.unwrap())?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;
    let _ = gather_complete.recv().await;

    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let s = json_str.clone();
        let b63 = signal::encode(&s);
        let firebase = Firebase::new("https://rust-signal-default-rtdb.europe-west1.firebasedatabase.app").unwrap().at("messages").at(&identify);
        let ans: Answer = Answer{answer: b63.clone()};
        firebase.update(&ans).await.unwrap();
        println!("[ANSWER]: OK");
    } else {
        println!("[ANSWER]: Err!");
    }

    tokio::select! {
        _ = done_rx.recv() => {
            println!("[SIGNAL]: DONE");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    };
    
    peer_connection.close().await?;

    Ok(())
}
