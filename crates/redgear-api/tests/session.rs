use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

// Sponge v3: floor lever -> wire -> lamp, plus a pressure plate below another lamp.
const CONTROLS: &str = "H4sIAAAAAAACCm2QQUvEMBSEp013y1ZEL/4BzxU87UHoRTyIeBAEPZRlCe1rG0yTkry1v9j/YdpVC2oOYfheZpJJBmTYPFcd9ZJVJZC+kPPKGgBC4OROsvwh5x8xVq+q5g5pjPU9qbZjiCAfybSBRhnWt9pWbz5D+iQ1MZPAaa8MVU42fCOVw5SMs4V5toYCjAS2C9X0Tq5sZEVFo611eZDKtIWxjrt8sCM5qotGak+7YI4FHhZzGM2p+1E5Kkl6LryqKZ/NIcLQMaG4zr09fKORvs7tjuUv/wnUsh9KrXi5ORG4+lVmPzjy/uCC0JKp/PPaFVIk09cGfRFFU/nQIJkG80qm7RN+de6cmwEAAA==";

struct Server {
    process: Child,
    address: SocketAddr,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}
impl Server {
    fn start() -> (Self, BufReader<TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let process = Command::new(env!("CARGO_BIN_EXE_mroww-redgear-api"))
            .arg(address.to_string())
            .env(
                "MCHPRS_LLVM_WORK_DIR",
                format!("work/api-tests/{}-{}", std::process::id(), address.port()),
            )
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let server = Self { process, address };
        let client = (0..150)
            .find_map(|_| match TcpStream::connect(address) {
                Ok(stream) => Some(stream),
                Err(_) => {
                    thread::sleep(Duration::from_millis(20));
                    None
                }
            })
            .expect("server did not start");
        client.set_read_timeout(Some(Duration::from_secs(90))).unwrap();
        (server, BufReader::new(client))
    }
    fn connect(&self) -> BufReader<TcpStream> {
        let stream = TcpStream::connect(self.address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(90))).unwrap();
        BufReader::new(stream)
    }
}

fn send(client: &mut BufReader<TcpStream>, request: Value) {
    writeln!(client.get_mut(), "{request}").unwrap();
    client.get_mut().flush().unwrap();
}
fn receive(client: &mut BufReader<TcpStream>) -> Value {
    let mut line = String::new();
    assert_ne!(client.read_line(&mut line).unwrap(), 0, "backend closed the connection");
    serde_json::from_str(&line).unwrap()
}
fn request(client: &mut BufReader<TcpStream>, request: Value) -> Value {
    let id = request.get("id").cloned();
    send(client, request);
    let response = receive(client);
    assert_eq!(response.get("id").cloned(), id, "{response}");
    response
}
fn begin_request(id: Option<u64>, backend: &str, settings: &str) -> Value {
    let mut value = json!({"op":"begin","protocol":4,"format":"sponge-v3","schematic":CONTROLS,"settings":settings,"backend":backend});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    value
}
fn finish(client: &mut BufReader<TcpStream>, session: &str) -> Value {
    loop {
        let response = receive(client);
        assert_eq!(response["session_id"], session, "{response}");
        match response["type"].as_str() {
            Some("progress") => {
                assert!(response["phase"].is_string(), "{response}");
                assert!(response["description"].is_string(), "{response}");
            }
            Some("finished") => return response,
            _ => panic!("unexpected compilation response: {response}"),
        }
    }
}
fn begin(client: &mut BufReader<TcpStream>, id: Option<u64>, backend: &str, settings: &str) -> (String, Value) {
    let accepted = request(client, begin_request(id, backend, settings));
    assert_eq!(accepted["type"], "accepted", "{accepted}");
    assert_eq!(accepted["protocol"], 4);
    assert_eq!(accepted["engine"], "mroww");
    let session = accepted["session_id"].as_str().unwrap().to_string();
    let finished = finish(client, &session);
    assert_eq!(finished["tick"], 0);
    (session, finished)
}
fn project(client: &mut BufReader<TcpStream>, session: &str, kind: &str) -> Value {
    let response = request(client, json!({"op":"project","session_id":session,"kind":kind}));
    assert_eq!(response["type"], "result", "{response}");
    response
}
fn advance(client: &mut BufReader<TcpStream>, session: &str, ticks: u64, inputs: Value, projection: bool) -> Value {
    let response = request(
        client,
        json!({"op":"advance","session_id":session,"ticks":ticks,"inputs":inputs,"projection":projection}),
    );
    assert_eq!(response["type"], "result", "{response}");
    response
}
fn states(response: &Value) -> BTreeMap<[i64; 3], String> {
    let palette = response["states"].as_array().unwrap();
    let mut result = BTreeMap::new();
    let mut previous = None;
    for entry in response["changes"].as_array().unwrap() {
        let entry = entry.as_array().unwrap();
        assert_eq!(entry.len(), 4);
        let pos = [entry[0].as_i64().unwrap(), entry[1].as_i64().unwrap(), entry[2].as_i64().unwrap()];
        assert!(previous.is_none_or(|previous| previous < pos), "changes must be unique and sorted");
        previous = Some(pos);
        let state = palette[entry[3].as_u64().unwrap() as usize].as_str().unwrap().to_string();
        result.insert(pos, state);
    }
    result
}
fn powered(response: &Value, pos: [i64; 3]) -> bool {
    states(response).get(&pos).is_some_and(|state| state.contains("powered=true"))
}

fn describe(client: &mut BufReader<TcpStream>, id: Option<u64>) -> Value {
    let mut value = json!({"op":"describe","protocol":4});
    if let Some(id) = id {
        value["id"] = json!(id);
    }
    let response = request(client, value);
    assert_eq!(response["type"], "manifest", "{response}");
    assert!(response.get("session_id").is_none());
    response
}

#[test]
fn discovery_advertises_selectable_backends_without_allocating_sessions() {
    let (_server, mut client) = Server::start();
    let manifest = describe(&mut client, Some(1));
    assert_eq!(manifest["engine"], "mroww");
    assert_eq!(manifest["protocol"], 4);
    assert_eq!(
        manifest["limits"],
        json!({"axis":64,"cells":30000,"ticks":1000000,"inputs":64,"message_bytes":16777216})
    );
    let mut expected = vec!["mroww-redpilator", "mroww-interpreter"];
    if cfg!(any(windows, target_os = "linux")) {
        expected.push("mroww-llvm");
    }
    let mut ids = manifest["backends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| b["id"].as_str().unwrap())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    expected.sort_unstable();
    assert_eq!(ids, expected);
    let default = if cfg!(any(windows, target_os = "linux")) {
        "mroww-llvm"
    } else {
        "mroww-redpilator"
    };
    assert_eq!(manifest["default_backend"], default);
    assert!(ids.contains(&default));
    for id in [Some(1), None, Some(u64::MAX)] {
        let mut repeated = describe(&mut client, id);
        repeated["id"] = json!(1);
        assert_eq!(repeated, manifest);
    }
    // Every advertised canonical selector works without client-side flag translation.
    for backend in manifest["backends"].as_array().unwrap() {
        let (session, finished) = begin(&mut client, Some(2), backend["id"].as_str().unwrap(), "");
        assert!(backend["executors"].as_array().unwrap().contains(&finished["backend"]));
        assert_eq!(backend["capabilities"]["recompile"], false);
        request(&mut client, json!({"op":"close","session_id":session}));
    }
    // Discovery neither consumes session numbers nor reserves any of the four slots.
    let mut sessions = Vec::new();
    for _ in 0..4 {
        sessions.push(begin(&mut client, None, "mroww-interpreter", "").0);
    }
    let first_number = manifest["backends"].as_array().unwrap().len() + 1;
    assert_eq!(sessions[0], format!("s{first_number}"));
    assert_eq!(describe(&mut client, Some(1)), manifest);
    let full = request(&mut client, begin_request(Some(2), "mroww-interpreter", ""));
    assert_eq!(full["code"], "resource_limit");
}

#[test]
fn discovery_errors_and_session_discovery_preserve_ticks_inputs_and_projections() {
    let (_server, mut client) = Server::start();
    for protocol in [json!(3), json!(5)] {
        let response = request(&mut client, json!({"id":1,"op":"describe","protocol":protocol}));
        assert_eq!(response["code"], "unsupported_protocol");
        assert!(response.get("session_id").is_none());
    }
    for protocol in [Value::Null, json!("4"), json!(4.5)] {
        let response = request(&mut client, json!({"id":1,"op":"describe","protocol":protocol}));
        assert_eq!(response["code"], "invalid_request");
    }
    assert_eq!(request(&mut client, json!({"op":"describe"}))["code"], "invalid_request");
    send(&mut client, json!({"id":"bad","op":"describe","protocol":4}));
    let malformed = receive(&mut client);
    assert_eq!(malformed["code"], "invalid_request");
    assert!(malformed.get("id").is_none());
    let manifest = describe(&mut client, Some(1));
    let (session, _) = begin(&mut client, Some(2), "mroww-interpreter", "");
    assert_eq!(session, "s1");
    project(&mut client, &session, "baseline");
    advance(&mut client, &session, 2, json!([{"pos":[0,1,0],"duration_ticks":3}]), false);
    assert_eq!(describe(&mut client, Some(1)), manifest);
    let delta = project(&mut client, &session, "changes");
    assert_eq!(delta["tick"], 2);
    assert!(powered(&delta, [0, 1, 0]), "discovery must not consume the pending projection");
    describe(&mut client, None);
    assert!(states(&project(&mut client, &session, "changes")).is_empty());
    let released = advance(&mut client, &session, 1, json!([]), true);
    assert_eq!(released["tick"], 3);
    assert!(states(&released)[&[0, 1, 0]].contains("powered=false"));
    for invalid in [
        json!({"op":"advance","session_id":session,"ticks":manifest["limits"]["ticks"].as_u64().unwrap()+1,"inputs":[]}),
        json!({"op":"advance","session_id":session,"ticks":0,"inputs":vec![json!({"pos":[0,1,0]}); manifest["limits"]["inputs"].as_u64().unwrap() as usize+1]}),
    ] {
        assert_eq!(request(&mut client, invalid)["type"], "error");
    }
    assert_eq!(project(&mut client, &session, "changes")["tick"], 3);
}

#[test]
fn discovery_during_preparation_obeys_outstanding_ids() {
    let (_server, mut client) = Server::start();
    send(&mut client, begin_request(Some(1), "mroww-redpilator", ""));
    let accepted = receive(&mut client);
    assert_eq!(accepted["type"], "accepted");
    // One write makes the reused ID arrive while preparation is still outstanding.
    writeln!(
        client.get_mut(),
        "{}\n{}",
        json!({"id":1,"op":"describe","protocol":4}),
        json!({"id":2,"op":"describe","protocol":4})
    )
    .unwrap();
    client.get_mut().flush().unwrap();
    let mut terminal = false;
    let mut duplicate = false;
    let mut manifest = false;
    while !terminal || !duplicate || !manifest {
        let value = receive(&mut client);
        match value["type"].as_str().unwrap() {
            "progress" => assert_eq!(value["session_id"], accepted["session_id"]),
            "finished" => {
                assert!(!terminal);
                terminal = true;
            }
            "error" => {
                assert_eq!(value["id"], 1);
                assert_eq!(value["code"], "invalid_request");
                duplicate = true;
            }
            "manifest" if value["id"] == 1 => {
                // A fast compilation may already have released ID 1 before discovery is read.
                assert!(terminal);
                duplicate = true;
            }
            "manifest" => {
                assert_eq!(value["id"], 2);
                assert!(value.get("session_id").is_none());
                manifest = true;
            }
            _ => panic!("{value}"),
        }
    }
}

#[test]
fn discovery_message_limit_includes_the_terminating_newline() {
    let (server, mut client) = Server::start();
    let manifest = describe(&mut client, None);
    let limit = manifest["limits"]["message_bytes"].as_u64().unwrap() as usize;
    let prefix = "{\"op\":\"describe\",\"protocol\":4,\"padding\":\"";
    let suffix = "\"}\n";
    let payload = format!("{prefix}{}{suffix}", "x".repeat(limit - prefix.len() - suffix.len()));
    assert_eq!(payload.len(), limit);
    client.get_mut().write_all(payload.as_bytes()).unwrap();
    assert_eq!(receive(&mut client), manifest);
    let mut oversized = server.connect();
    let _ = oversized.get_mut().write_all(payload.trim_end_matches('\n').as_bytes());
    let _ = oversized.get_mut().write_all(b" \n");
    let mut line = String::new();
    match oversized.read_line(&mut line) {
        Ok(count) => assert_eq!(count, 0),
        Err(e) => assert!(
            matches!(e.kind(), std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted),
            "{e}"
        ),
    }
    assert_eq!(describe(&mut client, None), manifest);
}

#[test]
fn v4_sessions_optional_ids_and_close_keep_connection_usable() {
    let (_server, mut client) = Server::start();
    let (first, finished) = begin(&mut client, Some(1), "redpilator", "");
    assert_eq!(finished["backend"], "rust");
    let (second, finished) = begin(&mut client, None, "mroww-interpteter", "");
    assert_eq!(finished["backend"], "interpreter");
    assert_ne!(first, second);
    let before = project(&mut client, &first, "baseline");
    assert!(states(&before).len() < 21, "baseline must omit unchanged cells");
    let closed = request(&mut client, json!({"id":2,"op":"close","session_id":first}));
    assert_eq!(closed["type"], "result");
    assert_eq!(closed["state"], "closed");
    let gone = request(&mut client, json!({"id":3,"op":"project","session_id":first,"kind":"baseline"}));
    assert_eq!(gone["code"], "unknown_session");
    assert_eq!(advance(&mut client, &second, 17, json!([]), false)["tick"], 17);
    let (third, finished) = begin(&mut client, Some(4), "mroww-interpreter", "");
    assert_eq!(finished["backend"], "interpreter");
    assert_ne!(second, third);
}

#[test]
fn timed_inputs_release_at_exact_boundaries_and_replacements_cancel_them() {
    let (_server, mut client) = Server::start();
    let (session, _) = begin(&mut client, Some(1), "mroww-redpilator", "");
    project(&mut client, &session, "baseline");
    let response = advance(&mut client, &session, 4, json!([{"pos":[0,1,0],"power":1,"duration_ticks":10}]), false);
    assert_eq!(response["tick"], 4);
    assert!(response.get("changes").is_none());
    assert!(powered(&project(&mut client, &session, "baseline"), [0, 1, 0]));
    advance(&mut client, &session, 5, json!([]), false);
    assert!(powered(&project(&mut client, &session, "baseline"), [0, 1, 0]));
    let released = advance(&mut client, &session, 1, json!([]), true);
    assert_eq!(released["tick"], 10);
    assert!(states(&released)[&[0, 1, 0]].contains("powered=false"));
    assert!(!powered(&project(&mut client, &session, "baseline"), [0, 1, 0]));

    advance(&mut client, &session, 0, json!([{"pos":[0,1,0],"duration_ticks":3}]), false);
    advance(&mut client, &session, 1, json!([]), false);
    advance(&mut client, &session, 0, json!([{"pos":[0,1,0]}]), false);
    advance(&mut client, &session, 5, json!([]), false);
    assert!(
        powered(&project(&mut client, &session, "baseline"), [0, 1, 0]),
        "omitting duration must cancel the old release"
    );
    advance(&mut client, &session, 0, json!([{"pos":[0,1,0],"power":0,"duration_ticks":1}]), false);
    advance(&mut client, &session, 1, json!([{"pos":[5,1,0],"duration_ticks":2}]), false);
    assert!(powered(&project(&mut client, &session, "baseline"), [5, 1, 0]));
    advance(&mut client, &session, 1, json!([]), false);
    assert!(!powered(&project(&mut client, &session, "baseline"), [5, 1, 0]));
}

#[test]
fn projections_compare_to_original_and_last_sent_state_even_with_io_publication() {
    let (_server, mut client) = Server::start();
    let (session, _) = begin(&mut client, Some(1), "redpilator", "--publish-io");
    let original = project(&mut client, &session, "baseline");
    assert!(states(&original).len() < 21);
    assert!(states(&project(&mut client, &session, "changes")).is_empty());
    advance(&mut client, &session, 1, json!([{"pos":[0,1,0]}]), false);
    let on = project(&mut client, &session, "changes");
    let changed = states(&on);
    assert!(changed[&[0, 1, 0]].contains("powered=true"));
    assert!(
        changed[&[1, 1, 0]].contains("power=15"),
        "IO publication must still project internal wire state"
    );
    assert!(changed[&[2, 1, 0]].contains("lit=true"));
    assert!(states(&project(&mut client, &session, "changes")).is_empty());
    advance(&mut client, &session, 5, json!([{"pos":[0,1,0],"power":0}]), false);
    let reverted = project(&mut client, &session, "changes");
    assert!(states(&reverted)[&[0, 1, 0]].contains("powered=false"));
    assert_eq!(states(&project(&mut client, &session, "baseline")), states(&original));
    assert!(states(&project(&mut client, &session, "changes")).is_empty());

    // Transient changes that return to the last published state are omitted.
    advance(&mut client, &session, 0, json!([{"pos":[0,1,0]}]), false);
    advance(&mut client, &session, 5, json!([{"pos":[0,1,0],"power":0}]), false);
    assert!(states(&project(&mut client, &session, "changes")).is_empty());
}

#[test]
fn invalid_inputs_and_unsupported_recompilation_do_not_mutate_ready_sessions() {
    let (_server, mut client) = Server::start();
    let (session, _) = begin(&mut client, None, "mroww-interpreter", "");
    let baseline = project(&mut client, &session, "baseline");
    for inputs in [
        json!([{"pos":[0,1,0]},{"pos":[1,1,0]}]),
        json!([{"pos":[0,1,0]},{"pos":[99,0,0]}]),
        json!([{"pos":[0,1,0],"power":16}]),
        json!([{"pos":[0,1,0],"duration_ticks":0}]),
        json!([{"pos":[0,1,0],"duration_ticks":-1}]),
        json!([{"pos":[0,1,0],"power":true}]),
        json!([{"pos":[0.5,1,0]}]),
    ] {
        let rejected = request(&mut client, json!({"id":9,"op":"advance","session_id":session,"ticks":1,"inputs":inputs}));
        assert_eq!(rejected["type"], "error", "{rejected}");
        let after = project(&mut client, &session, "baseline");
        assert_eq!(after["tick"], 0);
        assert_eq!(states(&after), states(&baseline));
    }
    let response = request(
        &mut client,
        json!({"id":10,"op":"recompile","session_id":session,"states":[],"changes":[]}),
    );
    assert_eq!(response["type"], "error");
    assert_eq!(advance(&mut client, &session, 1, json!([]), false)["tick"], 1);
}

#[test]
fn begin_validates_settings_and_reports_compile_errors_without_losing_other_sessions() {
    let (_server, mut client) = Server::start();
    for (backend, settings) in [
        ("redpilator", "--not-an-option"),
        ("mroww-redpilator", "--llvm"),
        ("mroww-interpreter", "--llvm"),
    ] {
        let error = request(&mut client, begin_request(Some(1), backend, settings));
        assert_eq!(error["type"], "error", "{error}");
        assert_eq!(error["code"], "invalid_settings", "{error}");
        assert!(error.get("session_id").is_none());
    }
    let (session, _) = begin(&mut client, Some(2), "mroww-interpreter", "");
    let mut invalid = begin_request(Some(3), "redpilator", "");
    invalid["schematic"] = json!("not-base64");
    let accepted = request(&mut client, invalid);
    if accepted["type"] == "accepted" {
        loop {
            let response = receive(&mut client);
            if response["type"] != "progress" {
                assert_eq!(response["type"], "error");
                assert_eq!(response["session_id"], accepted["session_id"]);
                break;
            }
        }
    } else {
        assert_eq!(accepted["type"], "error");
    }
    assert_eq!(advance(&mut client, &session, 2, json!([]), false)["tick"], 2);
}

#[test]
fn close_during_begin_orders_terminal_before_close_and_disconnect_releases_sessions() {
    let (server, mut client) = Server::start();
    let accepted = request(&mut client, begin_request(Some(1), "redpilator", ""));
    assert_eq!(accepted["type"], "accepted", "{accepted}");
    let session = accepted["session_id"].as_str().unwrap().to_string();
    send(&mut client, json!({"id":2,"op":"close","session_id":session}));
    let mut terminal = false;
    loop {
        let response = receive(&mut client);
        assert_eq!(response["session_id"], session);
        match response["type"].as_str() {
            Some("progress") => assert!(!terminal, "progress followed a terminal message"),
            Some("finished") => {
                assert!(!terminal);
                terminal = true;
            }
            Some("error") => {
                assert!(!terminal, "multiple terminal messages");
                assert_eq!(response["code"], "cancelled");
                terminal = true;
            }
            Some("result") => {
                assert!(terminal);
                assert_eq!(response["id"], 2);
                assert_eq!(response["state"], "closed");
                break;
            }
            _ => panic!("unexpected response: {response}"),
        }
    }
    // The connection remains usable after cancelling or closing compilation.
    begin(&mut client, Some(3), "mroww-interpreter", "");
    drop(client);

    // A disconnected compilation must not hold the service's client slots.
    for _ in 0..6 {
        let mut disconnecting = server.connect();
        send(&mut disconnecting, begin_request(None, "redpilator", ""));
        assert_eq!(receive(&mut disconnecting)["type"], "accepted");
        drop(disconnecting);
        thread::sleep(Duration::from_millis(30));
    }
    let started = Instant::now();
    let mut healthy = server.connect();
    begin(&mut healthy, None, "mroww-interpreter", "");
    assert!(started.elapsed() < Duration::from_secs(30));
}

#[test]
#[cfg(any(windows, target_os = "linux"))]
fn llvm_selector_runs_inputs_and_full_io_projection() {
    let (_server, mut client) = Server::start();
    let (session, finished) = begin(&mut client, Some(1), "mroww-llvm", "--publish-io");
    assert_eq!(finished["backend"], "llvm");
    let response = advance(&mut client, &session, 3, json!([{"pos":[0,1,0],"duration_ticks":4}]), true);
    assert_eq!(response["tick"], 3);
    assert!(powered(&response, [0, 1, 0]));
    assert!(states(&response)[&[1, 1, 0]].contains("power=15"));
    let released = advance(&mut client, &session, 1, json!([]), true);
    assert_eq!(released["tick"], 4);
    assert!(states(&released)[&[0, 1, 0]].contains("powered=false"));
}

#[test]
fn pipelined_close_answers_every_outstanding_request_once() {
    let (_server, mut client) = Server::start();
    let (session, _) = begin(&mut client, Some(1), "mroww-interpreter", "");
    let requests = [
        json!({"id":10,"op":"advance","session_id":session,"ticks":1_000_000,"inputs":[],"projection":false}),
        json!({"id":11,"op":"close","session_id":session}),
        json!({"id":12,"op":"project","session_id":session,"kind":"baseline"}),
        json!({"id":13,"op":"close","session_id":session}),
    ];
    let payload: String = requests.iter().map(|request| format!("{request}\n")).collect();
    client.get_mut().write_all(payload.as_bytes()).unwrap();
    client.get_mut().flush().unwrap();

    // Errors for later requests may arrive before the worker finishes closing.
    let mut responses = BTreeMap::new();
    for _ in 0..requests.len() {
        let response = receive(&mut client);
        assert_eq!(response["session_id"], session, "{response}");
        let id = response["id"].as_u64().unwrap();
        assert!((10..=13).contains(&id), "unexpected response: {response}");
        assert!(responses.insert(id, response).is_none(), "request {id} was answered twice");
    }
    let advanced = &responses[&10];
    match advanced["type"].as_str() {
        Some("result") => assert_eq!(advanced["tick"], 1_000_000),
        Some("error") => assert_eq!(advanced["code"], "execution_failed"),
        _ => panic!("unexpected advance response: {advanced}"),
    }
    assert_eq!(responses[&11]["type"], "result");
    assert_eq!(responses[&11]["state"], "closed");
    for id in [12, 13] {
        assert_eq!(responses[&id]["type"], "error");
        assert_eq!(responses[&id]["code"], "unknown_session");
    }
    // A duplicate late reply would also fail the next begin's response-ID check.
    let (next, finished) = begin(&mut client, Some(14), "mroww-interpreter", "");
    assert_ne!(next, session);
    assert_eq!(finished["backend"], "interpreter");
}
