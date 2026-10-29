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
fn lever_session_and_disconnect_cleanup() {
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
    assert_eq!(request(&mut client, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    for state in ["minecraft:unknown", "minecraft:lever[face=floor,facing=north]", "minecraft:lever[face=floor,facing=north,powered=false,extra=true]"] {
        let rejected = request(&mut client, json!({"id":100,"op":"load","size":[1,1,1],"data_version":5023,"boundary":"closed","palette":[state],"blocks":[[0,0,0,0]],"entities":[],"ticks":[]}));
        assert_eq!(rejected["ok"], false, "{state}");
    }
    let invalid_position = request(&mut client, json!({"id":101,"op":"load","size":[1,1,1],"data_version":4325,"boundary":"closed","palette":["minecraft:stone"],"blocks":[[1,0,0,0]],"entities":[],"ticks":[]}));
    assert_eq!(invalid_position["ok"], false);
    let pending_tick = request(&mut client, json!({"id":105,"op":"load","size":[1,1,1],"data_version":5023,"boundary":"closed","palette":["minecraft:stone"],"blocks":[[0,0,0,0]],"entities":[],"ticks":[{"pos":[0,0,0],"delay":1}]}));
    assert_eq!(pending_tick["ok"], false);
    assert!(pending_tick["error"].as_str().unwrap().contains("ticks"));
    assert_eq!(
        request(
            &mut client,
            json!({"id":2,"op":"load","size":[2,2,2],"data_version":5023,"boundary":"closed","palette":["minecraft:stone","minecraft:lever[powered=false,facing=north,face=floor]"],"blocks":[[0,0,0,0],[0,1,0,1]],"entities":[],"ticks":[]})
        )["ok"],
        true
    );
    let before = request(&mut client, json!({"id":102,"op":"inspect","pos":[0,1,0]}));
    assert_eq!(before["state"], "minecraft:lever[face=floor,facing=north,powered=false]");
    let rejected_reload = request(&mut client, json!({"id":103,"op":"load","size":[1,1,1],"data_version":4325,"boundary":"closed","palette":["minecraft:unknown"],"blocks":[[0,0,0,0]],"entities":[],"ticks":[]}));
    assert_eq!(rejected_reload["ok"], false);
    assert_eq!(request(&mut client, json!({"id":104,"op":"inspect","pos":[0,1,0]}))["state"], before["state"]);
    assert_eq!(request(&mut client, json!({"id":3,"op":"compile","mode":"interpreter"}))["ok"], true);
    let toggled = request(
        &mut client,
        json!({"id":4,"op":"advance","ticks":1,"inputs":[{"kind":"use","pos":[0,1,0]}],"projection":true}),
    );
    assert_eq!(toggled["ok"], true);
    assert_eq!(toggled["changes"].as_array().unwrap().len(), 1);
    let inspected = request(&mut client, json!({"id":5,"op":"inspect","pos":[0,1,0]}));
    assert!(inspected["state"].as_str().unwrap().contains("powered=true"));
    let drained = request(&mut client, json!({"id":6,"op":"project","kind":"changes"}));
    assert_eq!(drained["changes"].as_array().unwrap().len(), 0);
    let drained_again = request(&mut client, json!({"id":7,"op":"project","kind":"changes"}));
    assert_eq!(drained_again["changes"].as_array().unwrap().len(), 0);
    assert_eq!(
        request(
            &mut client,
            json!({"id":8,"op":"advance","ticks":0,"inputs":[{"kind":"use","pos":[9,0,0]}]})
        )["ok"],
        false
    );
    let after_invalid = request(&mut client, json!({"id":9,"op":"inspect","pos":[0,1,0]}));
    assert!(after_invalid["state"].as_str().unwrap().contains("powered=true"));
    assert_eq!(request(&mut client, json!({"id":10,"op":"close"}))["ok"], true);
    drop(client);
    let mut disconnected = BufReader::new(TcpStream::connect(address).unwrap());
    assert_eq!(request(&mut disconnected, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    assert_eq!(request(&mut disconnected, json!({"id":2,"op":"load","size":[1,1,1],"data_version":4325,"boundary":"closed","palette":[],"blocks":[],"entities":[],"ticks":[]}))["ok"], true);
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
    assert_eq!(request(&mut loaded, json!({"id":2,"op":"load","size":[1,1,1],"data_version":4325,"boundary":"closed","palette":[],"blocks":[],"entities":[],"ticks":[]}))["ok"], true);
    drop(loaded);
    let replacement = (0..100).find_map(|_| {
        let stream = TcpStream::connect(address).ok()?;
        stream.set_read_timeout(Some(Duration::from_millis(200))).ok()?;
        let mut reader = BufReader::new(stream);
        if writeln!(reader.get_mut(), "{}", json!({"id":1,"op":"rawr","protocol":3})).is_ok() {
            let mut line = String::new();
            if reader.read_line(&mut line).ok().filter(|count| *count > 0).is_some() {
                if serde_json::from_str::<Value>(&line).ok().is_some_and(|v| v["ok"] == true) { return Some(reader); }
            }
        }
        thread::sleep(Duration::from_millis(20));
        None
    }).expect("disconnected session slot was not released");
    drop(replacement);
    drop(clients);
    thread::sleep(Duration::from_millis(50));

    let mut oversized = TcpStream::connect(address).unwrap();
    oversized.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
    let _ = oversized.write_all(&vec![b'x'; 1_048_577]);
    let mut byte = [0];
    match oversized.peek(&mut byte) {
        Ok(count) => assert_eq!(count, 0),
        Err(error) => assert!(matches!(error.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted)),
    }
    let mut healthy = BufReader::new(TcpStream::connect(address).unwrap());
    assert_eq!(request(&mut healthy, json!({"id":1,"op":"rawr","protocol":3}))["ok"], true);
    assert_eq!(request(&mut healthy, json!({"id":2,"op":"close"}))["ok"], true);
}
