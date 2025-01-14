use std::time::Duration;

use tokio::sync::broadcast::Sender;
use tokio::sync::{broadcast, oneshot};

use async_uws::app::App;
use async_uws::data_storage::DataStorage;
use async_uws::http_request::HttpRequest;
use async_uws::http_response::HttpResponse;
use async_uws::uwebsockets_rs::CompressOptions;
use async_uws::uwebsockets_rs::Opcode;
use async_uws::uwebsockets_rs::UsSocketContextOptions;
use async_uws::websocket::Websocket;
use async_uws::ws_behavior::WsRouteSettings;
use async_uws::ws_message::WsMessage;

#[derive(Clone)]
struct SharedData {
    pub data: String,
}

#[tokio::main]
async fn main() {
    let opts = UsSocketContextOptions {
        key_file_name: None,
        cert_file_name: None,
        passphrase: None,
        dh_params_file_name: None,
        ca_file_name: None,
        ssl_ciphers: None,
        ssl_prefer_low_memory_usage: None,
    };

    let shared_data = SharedData {
        data: "String containing data".to_string(),
    };

    let (sink, stream) = oneshot::channel::<()>();
    let (b_sink, mut b_stream) = broadcast::channel::<()>(1);
    tokio::spawn(async move {
        let _ = b_stream.recv().await;
        sink.send(()).unwrap();
    });

    let mut app = App::new(opts, Some(stream));
    let compressor: u32 = CompressOptions::SharedCompressor.into();
    let decompressor: u32 = CompressOptions::SharedDecompressor.into();
    let route_settings = WsRouteSettings {
        compression: Some(compressor | decompressor),
        max_payload_length: Some(1024),
        idle_timeout: Some(800),
        max_backpressure: Some(10),
        close_on_backpressure_limit: Some(false),
        reset_idle_timeout_on_send: Some(true),
        send_pings_automatically: Some(true),
        max_lifetime: Some(111),
    };
    app.data(shared_data);
    app.data(b_sink);

    app.ws(
        "/ws",
        route_settings.clone(),
        |mut ws| async move {
            let b_sink = ws.data::<Sender<()>>().unwrap().clone();
            let status = ws.send("hello".into()).await;
            println!("Send status: {status:#?}");

            while let Some(msg) = ws.stream.recv().await {
                println!("{msg:#?}");
                if let WsMessage::Message(data, _) = msg {
                    println!("{data:#?}");
                    b_sink.send(()).unwrap();
                };
                let status = ws
                    .send(WsMessage::Message(
                        "asdfasdf".as_bytes().to_vec(),
                        Opcode::Text,
                    ))
                    .await;
                println!("{status:#?}");
            }
        },
        |req, res| {
            custom_upgrade(req, res);
        },
    )
    .ws(
        "/ws-test",
        route_settings.clone(),
        handler_ws,
        custom_upgrade,
    )
    .ws(
        "/split",
        route_settings,
        ws_split,
        HttpResponse::default_upgrade,
    )
    .listen(
        3001,
        Some(|listen_socket| {
            println!("{listen_socket:#?}");
        }),
    )
    .run();
    println!("Server exiting");
}

fn custom_upgrade(mut req: HttpRequest, res: HttpResponse<false>) {
    let ws_key_string = req
        .get_header("sec-websocket-key")
        .expect("[async_uws]: There is no sec-websocket-key in req headers")
        .to_string();
    let ws_protocol = req.get_header("sec-websocket-protocol").map(String::from);
    let ws_extensions = req.get_header("sec-websocket-extensions").map(String::from);

    let full_url = req.get_full_url().to_string();
    let headers: Vec<(String, String)> = req
        .get_headers()
        .clone()
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();

    let upgrade_req_info = UpgradeReqInfo { full_url, headers };
    let mut connection_data_storage = DataStorage::new();

    connection_data_storage.add_data(upgrade_req_info);

    res.upgrade(
        ws_key_string,
        ws_protocol,
        ws_extensions,
        Some(connection_data_storage.into()),
    );
}

#[derive(Debug, Clone)]
struct UpgradeReqInfo {
    full_url: String,
    headers: Vec<(String, String)>,
}
async fn handler_ws(mut ws: Websocket<false>) {
    let data = ws.data::<SharedData>().unwrap();
    println!("!!! Global Shared data: {}", data.data);
    let per_connection_data = ws.connection_data::<UpgradeReqInfo>().unwrap();
    println!(
        "!!! Upgrade url: {:#?}, headers: {:#?}",
        per_connection_data.full_url, per_connection_data.headers
    );

    while let Some(msg) = ws.stream.recv().await {
        match msg {
            WsMessage::Message(bin, opcode) => {
                if opcode == Opcode::Text {
                    let msg = String::from_utf8(bin).unwrap();
                    println!("{msg}");

                    if msg.contains("close") {
                        ws.send(WsMessage::Close(1003, Some("just close".to_string())))
                            .await
                            .unwrap();
                    }
                }
            }
            WsMessage::Ping(_) => {
                println!("Got ping");
            }
            WsMessage::Pong(_) => {
                println!("Got pong");
            }
            WsMessage::Close(code, reason) => {
                println!("Got close: {code}, {reason:#?}");
                break;
            }
        }
        ws.send(WsMessage::Message(
            Vec::from("response to your message".as_bytes()),
            Opcode::Text,
        ))
        .await
        .unwrap();
    }
    println!("Done with that websocket!");
}

async fn ws_split(ws: Websocket<false>) {
    let (sink, mut stream) = ws.split();
    tokio::spawn(async move {
        loop {
            if let Err(e) = sink.send(("Hello! I'm timer".into(), false, true)) {
                println!("Error send to socket:{e:#?}");
                break;
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    while let Some(message) = stream.recv().await {
        println!("Incoming: {message:#?}")
    }
}
