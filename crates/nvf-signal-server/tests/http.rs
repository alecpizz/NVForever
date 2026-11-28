//! The server on real sockets: the REST calls, then a host and a guest over WebSockets through
//! a join, with the same messages the Worker's conformance script checks.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use nvf_signal_server::{deployment_config, Server};
use tungstenite::{connect, Message};

fn http(addr: std::net::SocketAddr, method: &str, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    write!(s, "{method} {path} HTTP/1.1\r\nhost: x\r\ncontent-length: {}\r\n\r\n{body}", body.len()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out[9..12].parse().unwrap();
    let body = out.split_once("\r\n\r\n").unwrap().1.to_string();
    (status, body)
}

fn json(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap()
}

type Ws = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>;

fn ws(addr: std::net::SocketAddr, code: &str) -> Ws {
    let (w, _) = connect(format!("ws://{addr}/v1/lobbies/{code}/ws")).unwrap();
    w
}

fn next(w: &mut Ws) -> serde_json::Value {
    loop {
        if let Message::Text(t) = w.read().unwrap() {
            return json(t.as_str());
        }
    }
}

fn send(w: &mut Ws, v: serde_json::Value) {
    w.send(Message::text(v.to_string())).unwrap();
}

#[test]
fn a_join_through_the_server() {
    let server = Server::start("127.0.0.1:0".parse().unwrap(), deployment_config(138, vec!["stun.example:3478".into()])).unwrap();
    let at = server.addr;
    let key = |n: u8| format!("{n:02x}").repeat(32);
    let create = serde_json::json!({"wire":1,"protocol":138,"build":"0.8.1","load_order_hash":"abc","max_players":3,"host_key":key(1),"name":"Courier"});

    let (status, body) = http(at, "POST", "/v1/lobbies", &create.to_string());
    assert_eq!(status, 200, "{body}");
    let lobby = json(&body);
    let code = lobby["code"].as_str().unwrap().to_string();
    assert_eq!(lobby["stun"][0], "stun.example:3478");
    let mut old = create.clone();
    old["protocol"] = 1.into();
    assert_eq!(http(at, "POST", "/v1/lobbies", &old.to_string()).0, 403);
    let typed = format!("{}-{}", &code[..3], &code[3..]).to_lowercase();
    let (status, body) = http(at, "GET", &format!("/v1/lobbies/{typed}"), "");
    assert_eq!((status, json(&body)["players"].as_u64()), (200, Some(1)));
    assert_eq!(http(at, "GET", "/v1/lobbies/ZZZ-ZZZ", "").0, 404);
    assert_eq!(http(at, "GET", "/nope", "").0, 404);
    assert_eq!(server.lobbies(), 1);

    let mut host = ws(at, &code);
    send(&mut host, serde_json::json!({"type":"host","wire":1,"host_secret":lobby["host_secret"]}));
    assert_eq!(next(&mut host)["type"], "hosting");
    let mut guest = ws(at, &typed);
    send(&mut guest, serde_json::json!({"type":"join","wire":1,"name":"Six","key":key(6),"candidates":[{"kind":"host","addr":"192.168.2.2:47960"}]}));
    assert_eq!(next(&mut guest), serde_json::json!({"type":"waiting","peer":1}));
    let m = next(&mut host);
    assert_eq!((m["type"].as_str(), m["key"].as_str()), (Some("join"), Some(key(6).as_str())));
    send(&mut host, serde_json::json!({"type":"accept","peer":1,"candidates":[{"kind":"relay","addr":"198.51.100.9:47961"}]}));
    assert_eq!(next(&mut host), serde_json::json!({"type":"go","peer":1}));
    let m = next(&mut guest);
    assert_eq!(m["type"], "accepted");
    assert_eq!(m["candidates"][0]["kind"], "relay");
    assert_eq!(next(&mut guest)["type"], "go");
    send(&mut guest, serde_json::json!({"type":"connected","path":"direct"}));
    assert_eq!(next(&mut host), serde_json::json!({"type":"connected","peer":1,"path":"direct"}));

    // A guest's socket closing reaches the host.
    guest.close(None).unwrap();
    let _ = guest.flush();
    assert_eq!(next(&mut host), serde_json::json!({"type":"left","peer":1}));
    // The host's bye ends the lobby.
    send(&mut host, serde_json::json!({"type":"bye"}));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(http(at, "GET", &format!("/v1/lobbies/{code}"), "").0, 404);
    assert_eq!(server.lobbies(), 0);
}

#[test]
fn browse_and_report_over_http() {
    let server = Server::start("127.0.0.1:0".parse().unwrap(), deployment_config(0, vec![])).unwrap();
    let at = server.addr;
    let key = format!("{:02x}", 7).repeat(32);
    let create = serde_json::json!({"wire":2,"protocol":139,"build":"0.8.1","load_order_hash":"abc","max_players":4,"host_key":key,"name":"Courier",
        "listing":{"public":true,"title":"Strip run","place":"The Strip","friendly_fire":false,"plugins":3,"locked":false}});
    let lobby = json(&http(at, "POST", "/v1/lobbies", &create.to_string()).1);
    let code = lobby["code"].as_str().unwrap().to_string();
    // Listed once its host is connected.
    assert_eq!(json(&http(at, "GET", "/v1/browse", "").1)["total"], 0);
    let mut host = ws(at, &code);
    send(&mut host, serde_json::json!({"type":"host","wire":2,"host_secret":lobby["host_secret"]}));
    assert_eq!(next(&mut host)["type"], "hosting");
    let (status, body) = http(at, "GET", "/v1/browse?q=strip%20run&protocol=139", "");
    assert_eq!(status, 200, "{body}");
    let b = json(&body);
    assert_eq!((b["total"].as_u64(), b["lobbies"][0]["code"].as_str(), b["lobbies"][0]["host_fp"].as_str()), (Some(1), Some(code.as_str()), Some(&key[..16])));
    assert_eq!(json(&http(at, "GET", "/v1/browse?protocol=1", "").1)["total"], 0);
    assert_eq!(http(at, "POST", &format!("/v1/lobbies/{code}/report"), "{}"), (200, r#"{"ok":true}"#.to_string()));
    assert_eq!(http(at, "POST", "/v1/lobbies/ZZZZZZ/report", "{}").0, 404);
}

#[test]
fn deployment_secrets_come_from_the_os() {
    let a = nvf_signal_server::os_random();
    let b = nvf_signal_server::os_random();
    assert_ne!(a, b);
}
