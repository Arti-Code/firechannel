use anyhow::Result;
use clap::{App, Arg, value_t};
use tokio::time::{sleep, Duration};
use firebase_rs::*;
use webrtc::api::media_engine::MediaEngine;
use clap::{AppSettings};
use std::io::Write;
use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::APIBuilder;
use webrtc::data_channel::data_channel_message::DataChannelMessage;
use webrtc::data_channel::RTCDataChannel;
use webrtc::ice_transport::ice_server::RTCIceServer;
use webrtc::interceptor::registry::{Registry, self};
use webrtc::peer_connection::configuration::RTCConfiguration;
use webrtc::peer_connection::{math_rand_alpha, self};
use webrtc::peer_connection::peer_connection_state::RTCPeerConnectionState;
use webrtc::peer_connection::sdp::session_description::RTCSessionDescription;

#[tokio::main]
async fn main() -> Result<()> {
    let app = App::new("fire-channel")
        .version("0.1.0")
        .author("Artur Gwoździowski <gwozdyk@gmail.com>")
        .about("an example of creation p2p data-channel using firebase as signaling")
        .arg(
            Arg::new("IDENT")
                .help("some kind of callsign to identifying peer and data-channel name")
                .long("ident")
                .short('i')
                .takes_value(true)
        )
        .get_matches();

    if !app.is_present("IDENT") {
        println!("please provide an unique identifier (-ident <ident>)/(--i <ident>)");
        std::process::exit(1);
    }

    let identify = value_t!(app, "IDENT", String).unwrap();
    println!("peer unique ident is: {}", identify);
    println!("connecting");
    println!("please wait a moment...");
    sleep(Duration::from_secs(3)).await;
    let firebase = Firebase::new("https://rust-signal-default-rtdb.europe-west1.firebasedatabase.app")
        .unwrap()
        .at("negotiations")
        .at(&identify)
        .at("offer");
    let offer = firebase.get::<String>().await;
    println!("[OFFER]: {:?}", offer);

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

    peer_connection
        .on_peer_connection_state_change(Box::new(move |s: RTCPeerConnectionState| {
            println!("Peer Connection State has changed: {}", s);
            if s == RTCPeerConnectionState::Failed {
                println!("Peer Connection has gone to failed exiting");
                let _ = done_tx.try_send(());
            }
            Box::pin(async {})
        }))
        .await;

    peer_connection
        .on_data_channel(Box::new(move |d: Arc<RTCDataChannel>| {
            let d_label = d.label().to_owned();
            let d_id = d.id();
            println!("New DataChannel {} {}", d_label, d_id);

            Box::pin(async move {
                let d2 = Arc::clone(&d);
                let d_label2 = d_label.clone();
                let d_id2 = d_id;
                d.on_open(Box::new(move || {
                    println!("Data channel '{}'-'{}' open. Random messages will now be sent to any connected DataChannels every 5 seconds", d_label2, d_id2);
                    Box::pin(async move {
                        let mut result = Result::<usize>::Ok(0);
                        while result.is_ok() {
                            let timeout = tokio::time::sleep(Duration::from_secs(5));
                            tokio::pin!(timeout);

                            tokio::select! {
                                _ = timeout.as_mut() => {
                                    let message = math_rand_alpha(15);
                                    println!("Sending '{}'", message);
                                    result = d2.send_text(message).await.map_err(Into::into);
                                }
                            };
                        }
                    })
                })).await;

                d.on_message(Box::new(move |msg: DataChannelMessage| {
                    let msg_str = String::from_utf8(msg.data.to_vec()).unwrap();
                    println!("Message from DataChannel '{}': '{}'", d_label, msg_str);
                    Box::pin(async {})
                })).await;
            })
        }))
        .await;

    let line = signal::must_read_stdin()?;
    let desc_data = signal::decode(line.as_str())?;
    let offer = serde_json::from_str::<RTCSessionDescription>(&desc_data)?;
    peer_connection.set_remote_description(offer).await?;
    let answer = peer_connection.create_answer(None).await?;
    let mut gather_complete = peer_connection.gathering_complete_promise().await;
    peer_connection.set_local_description(answer).await?;
    let _ = gather_complete.recv().await;

    if let Some(local_desc) = peer_connection.local_description().await {
        let json_str = serde_json::to_string(&local_desc)?;
        let b64 = signal::encode(&json_str);
        println!("{}", b64);
    } else {
        println!("generate local_description failed!");
    }

    println!("Press ctrl-c to stop");
    tokio::select! {
        _ = done_rx.recv() => {
            println!("received done signal!");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("");
        }
    };
    peer_connection.close().await?;

    Ok(())
}
