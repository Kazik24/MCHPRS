use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::Duration;

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn request(reader: &mut BufReader<TcpStream>, request: Value) -> Value {
    writeln!(reader.get_mut(), "{request}").unwrap();
    reader.get_mut().flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn schematic_loading_and_disconnect_cleanup() {
    let socket = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    let _server = Server(
        Command::new(env!("CARGO_BIN_EXE_mroww-redgear-api"))
            .arg(address.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let stream = (0..100)
        .find_map(|_| match TcpStream::connect(address) {
            Ok(stream) => Some(stream),
            Err(_) => {
                thread::sleep(Duration::from_millis(20));
                None
            }
        })
        .expect("server did not start");
    stream.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    let mut client = BufReader::new(stream);
    let hello = request(&mut client, json!({"id":1,"op":"rawr","protocol":3}));
    assert_eq!(hello["ok"], true);
    assert_eq!(hello["capabilities"]["schematic_load"], true);
    use base64::Engine;
    let schematic = base64::engine::general_purpose::STANDARD.encode(include_bytes!("../../../test_data/graph-fixtures/simple_or_gate.schem"));
    let load = json!({"id":2,"op":"load_schematic","format":"sponge-v3","schematic":schematic});
    let invalid = request(&mut client, json!({"id":100,"op":"load_schematic","format":"sponge-v3","schematic":"invalid-base64"}));
    assert_eq!(invalid["ok"], false);
    assert_eq!(request(&mut client, load.clone())["ok"], true);
    assert_eq!(request(&mut client, json!({"id":3,"op":"compile","mode":"interpreter"}))["ok"], true);
    let baseline = request(&mut client, json!({"id":4,"op":"project","kind":"baseline"}));
    assert_eq!(baseline["ok"], true);
    assert_eq!(baseline["changes"].as_array().unwrap().len(), 36);
    let first = &baseline["changes"][0];
    let expected = &baseline["states"][first[3].as_u64().unwrap() as usize];
    assert_eq!(request(&mut client, json!({"id":5,"op":"inspect","pos":[first[0],first[1],first[2]]}))["state"], *expected);
    assert_eq!(request(&mut client, load.clone())["ok"], false);
    assert_eq!(request(&mut client, json!({"id":6,"op":"inspect","pos":[first[0],first[1],first[2]]}))["state"], *expected);
    for id in [7, 8] {
        let drained = request(&mut client, json!({"id":id,"op":"project","kind":"changes"}));
        assert_eq!(drained["ok"], true);
        assert!(drained["changes"].as_array().unwrap().is_empty());
    }
    assert_eq!(request(&mut client, json!({"id":9,"op":"advance","ticks":0,"inputs":[{"kind":"use","pos":[99,0,0]}]}))["ok"], false);
    assert_eq!(request(&mut client, json!({"id":10,"op":"close"}))["ok"], true);
    drop(client);
    let mut disconnected = BufReader::new(TcpStream::connect(address).unwrap());
    assert_eq!(request(&mut disconnected, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    assert_eq!(request(&mut disconnected, load.clone())["ok"], true);
    drop(disconnected);
    let mut next = BufReader::new(TcpStream::connect(address).unwrap());
    assert_eq!(request(&mut next, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    assert_eq!(request(&mut next, json!({"id":2,"op":"close"}))["ok"], true);
    drop(next);
    thread::sleep(Duration::from_millis(50));

    let mut clients = Vec::new();
    for _ in 0..4 {
        let mut client = BufReader::new(TcpStream::connect(address).unwrap());
        client.get_ref().set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        assert_eq!(request(&mut client, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
        clients.push(client);
    }
    let mut loaded = clients.pop().unwrap();
    assert_eq!(
        request(
            &mut loaded,
            load.clone()
        )["ok"],
        true
    );
    drop(loaded);
    let replacement = (0..100)
        .find_map(|_| {
            let stream = TcpStream::connect(address).ok()?;
            stream.set_read_timeout(Some(Duration::from_millis(200))).ok()?;
            let mut reader = BufReader::new(stream);
            if writeln!(reader.get_mut(), "{}", json!({"id":1,"op":"rawr","protocol":3})).is_ok() {
                let mut line = String::new();
                if reader.read_line(&mut line).ok().filter(|count| *count > 0).is_some() {
                    if serde_json::from_str::<Value>(&line).ok().is_some_and(|v| v["ok"] == true) {
                        return Some(reader);
                    }
                }
            }
            thread::sleep(Duration::from_millis(20));
            None
        })
        .expect("disconnected session slot was not released");
    drop(replacement);
    drop(clients);
    thread::sleep(Duration::from_millis(50));

    let mut oversized = TcpStream::connect(address).unwrap();
    oversized.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let _ = oversized.write_all(&vec![b'x'; 1_048_577]);
    let mut byte = [0];
    match oversized.peek(&mut byte) {
        Ok(count) => assert_eq!(count, 0),
        Err(error) => assert!(matches!(
            error.kind(),
            std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted
        )),
    }
    let mut healthy = BufReader::new(TcpStream::connect(address).unwrap());
    assert_eq!(request(&mut healthy, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    assert_eq!(request(&mut healthy, json!({"id":2,"op":"close"}))["ok"], true);
}
