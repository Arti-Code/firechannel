use anyhow::Result;
use clap::{App, Arg, value_t};
use tokio::time::{sleep, Duration};
use firebase_rs::*;
use webrtc::api::media_engine::MediaEngine;
use clap::{AppSettings};
use std::io::Write;
use std::sync::Arc;
use webrtc::api::interceptor_registry::register_default_interceptors;
use webrtc::api::media_engine::MediaEngine;
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
        .on_peer_connection_state_change(Box::new(move /s: RTCPeerConnectionState/ {
            println!("Peer Connection State has changed: {}", s);
            
        }))
        .await;

    Ok(())
}
