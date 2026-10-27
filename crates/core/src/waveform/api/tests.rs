use super::*;
use std::io::Read as _;

#[test]
fn routes_reject_invalid_coordinates_queries_and_cross_plot_cursors() {
    for uri in [
        "/api/v1/plots/01/2/waveform",
        "/api/v1/plots/2147483648/2/waveform",
        "/api/v1/plots/1/2/waveform/data?limit=99999",
        "/api/v1/plots/1/2/waveform/data?cursor=bad",
        "/api/v1/plots/1/2/waveform/stream",
    ] {
        assert!(route(&Request::get(uri).body(Body::empty()).unwrap()).is_err(), "{uri}");
    }
    assert!(route(&Request::get("/api/v1/plots/-3/2/waveform/data?limit=1").body(Body::empty()).unwrap()).is_ok());
}
#[test]
fn http_snapshot_and_sse_resume_without_loading_or_blocking_simulation() {
    let api = ApiServer::start("127.0.0.1:0").unwrap();
    let client = reqwest::blocking::Client::builder().timeout(Duration::from_secs(5)).build().unwrap();
    let url = format!("http://{}/api/v1/plots/987654/1/waveform", api.address);
    assert_eq!(client.get(&url).send().unwrap().status().as_u16(), 404);
    let mut source = Source::register((987654, 1)).unwrap();
    let (stop_send, stop_recv) = mpsc::channel();
    let (text_send, text_recv) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let mut recorder = Recorder::default();
        recorder.on(1000, 0, vec![], 64).unwrap();
        while stop_recv.try_recv().is_err() {
            if text_recv.try_recv().is_ok() {
                recorder.text(mchprs_blocks::BlockPos::new(4, 64, 5), r#"{"text":"ready"}"#, 0);
            }
            source.poll(&recorder, false, None);
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let snapshot: Value = client.get(format!("{url}/data")).send().unwrap().json().unwrap();
    assert!(snapshot.get("owner").unwrap().is_null());
    let cursor = snapshot["cursor"].as_str().unwrap();
    let mut stream = client.get(format!("{url}/stream")).query(&[("cursor", cursor)]).send().unwrap();
    assert_eq!(stream.status().as_u16(), 200);
    assert_eq!(stream.headers()["content-type"], "text/event-stream");
    let mut buffer = [0; 4096];
    let len = stream.read(&mut buffer).unwrap();
    let text = std::str::from_utf8(&buffer[..len]).unwrap();
    assert!(text.contains("event: batch"));
    assert!(text.contains("id: 1:"));
    text_send.send(()).unwrap();
    let mut emitted = String::new();
    while !emitted.contains("ready") {
        let len = stream.read(&mut buffer).unwrap();
        assert_ne!(len, 0);
        emitted.push_str(std::str::from_utf8(&buffer[..len]).unwrap());
    }
    assert!(emitted.contains("[1,1,4,64,5,\"ready\",0]"));
    drop(stream);
    stop_send.send(()).unwrap();
    worker.join().unwrap();
    assert_eq!(client.get(&url).send().unwrap().status().as_u16(), 404);
    drop(api);
}
