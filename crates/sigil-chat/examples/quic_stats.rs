//! Fetch chunks of a blob over a raw sQUIC+h3 connection and print quinn's
//! path statistics -- loss, cwnd, RTT, MTU -- after each phase, then put the
//! same many bytes up and abort the upload. A bench tool, not a feature: it
//! is how the transport's per-connection ceiling was measured, and the only
//! place the window is visible from the client side.
//!
//!   cargo run --release -p sigil-chat --example quic_stats -- <identity> <host:port> <server-key> <blob> <chunks>
//!
//! `TRACE=1` fetches one chunk and prints packets received per 20 ms -- the
//! server's pacing made visible. `BBR=1` dials with BBR instead of Cubic,
//! which governs the upload direction only.
use std::time::Instant;

use bytes::Buf as _;
use sqnr_core::PubKey;

#[tokio::main]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let signer = sqnr::identity::load(std::path::Path::new(&a[1]), None).unwrap();
    let seed = signer.seed();
    let addr: std::net::SocketAddr = {
        use std::net::ToSocketAddrs as _;
        a[2].to_socket_addrs().unwrap().next().unwrap()
    };
    let server = PubKey::from_base58(&a[3]).unwrap();
    // `-` for the blob means: put one up first (17 chunks of 256 KiB, committed
    // under its real name) and fetch that, for an exchange that holds nothing.
    let chunks: u32 = a[5].parse().unwrap();
    let mut blob: [u8; 32] = if a[4] == "-" {
        [0; 32]
    } else {
        bs58::decode(&a[4]).into_vec().unwrap().try_into().unwrap()
    };
    let conn = squic::dial(
        addr,
        server.as_bytes(),
        squic::Config {
            alpn_protocols: vec![b"h3".to_vec()],
            keep_alive: Some(std::time::Duration::from_secs(15)),
            handshake_timeout: Some(std::time::Duration::from_secs(5)),
            client_key: Some(hex::encode(seed)),
            advertise_identity: true,
            enable_datagrams: true,
            congestion_controller: if std::env::var("BBR").is_ok() {
                squic::CongestionController::Bbr
            } else {
                squic::CongestionController::Cubic
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let raw = conn.clone();
    let (mut driver, send) = h3::client::new(h3_quinn::Connection::new(conn))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = driver.wait_idle().await;
    });
    let post = |mut send: h3::client::SendRequest<h3_quinn::OpenStreams, bytes::Bytes>,
                body: Vec<u8>| async move {
        let req = http::Request::builder()
            .method("POST")
            .uri("https://sqex/blob/get")
            .body(())
            .unwrap();
        let mut stream = send.send_request(req).await.unwrap();
        stream.send_data(bytes::Bytes::from(body)).await.unwrap();
        stream.finish().await.unwrap();
        let resp = stream.recv_response().await.unwrap();
        assert_eq!(resp.status().as_u16(), 200);
        let mut n = 0;
        while let Some(mut chunk) = stream.recv_data().await.unwrap() {
            n += chunk.remaining();
            chunk.advance(chunk.remaining());
        }
        n
    };
    // Upload: the client's own controller is what governs this direction.
    let begin = |mut send: h3::client::SendRequest<h3_quinn::OpenStreams, bytes::Bytes>,
                 path: String,
                 body: Vec<u8>| async move {
        let req = http::Request::builder()
            .method("POST")
            .uri(format!("https://sqex{path}"))
            .body(())
            .unwrap();
        let mut stream = send.send_request(req).await.unwrap();
        stream.send_data(bytes::Bytes::from(body)).await.unwrap();
        stream.finish().await.unwrap();
        let resp = stream.recv_response().await.unwrap();
        let status = resp.status().as_u16();
        let mut out = Vec::new();
        while let Some(mut chunk) = stream.recv_data().await.unwrap() {
            out.extend_from_slice(chunk.chunk());
            chunk.advance(chunk.remaining());
        }
        (status, out)
    };
    let print = |label: &str, raw: &quinn::Connection| {
        let s = raw.stats();
        println!(
            "{label}: rtt {:?} cwnd {} mtu {} | sent {} pkts lost {} pkts ({} bytes) congestion events {} | recv {} pkts | black holes {}",
            s.path.rtt,
            s.path.cwnd,
            s.path.current_mtu,
            s.path.sent_packets,
            s.path.lost_packets,
            s.path.lost_bytes,
            s.path.congestion_events,
            s.udp_rx.datagrams,
            s.path.black_holes_detected,
        );
    };
    print("after handshake", &raw);
    if a[4] == "-" {
        let (code, body) = begin(
            send.clone(),
            "/channel/mine".into(),
            sqex_proto::channel::Mine { offset: 0 }.encode(),
        )
        .await;
        assert_eq!(code, 200);
        let channel = sqex_proto::channel::Mines::decode(&body).unwrap().channels[0].channel;
        let sealed: Vec<Vec<u8>> = (0..chunks)
            .map(|i| {
                (0..256 * 1024)
                    .map(|j| ((j * 7 + i as usize) % 251) as u8)
                    .collect()
            })
            .collect();
        blob = sqex_proto::blob_store::blob_id(&sealed);
        let (code, body) = begin(
            send.clone(),
            "/blob/begin".into(),
            sqex_proto::blob_store::Begin {
                channel,
                size: chunks as u64 * 256 * 1024,
                chunks,
                expires_after: 0,
            }
            .encode(),
        )
        .await;
        assert_eq!(code, 200, "{}", String::from_utf8_lossy(&body));
        let upload = sqex_proto::blob_store::Begun::decode(&body).unwrap().upload;
        for (index, sealed) in sealed.into_iter().enumerate() {
            let (code, _) = begin(
                send.clone(),
                "/blob/put".into(),
                sqex_proto::blob_store::PutChunk {
                    upload,
                    index: index as u32,
                    sealed,
                }
                .encode(),
            )
            .await;
            assert_eq!(code, 200);
        }
        let (code, body) = begin(
            send.clone(),
            "/blob/commit".into(),
            sqex_proto::blob_store::Commit { upload, blob }.encode(),
        )
        .await;
        assert_eq!(code, 200);
        assert!(
            sqex_proto::blob_store::Committed::decode(&body)
                .unwrap()
                .stored
        );
        println!(
            "put up {chunks} chunks as {}",
            bs58::encode(blob).into_string()
        );
        print("after seeding", &raw);
    }
    if std::env::var("TRACE").is_ok() {
        // One chunk, with the arrival pattern: how many packets landed in
        // each 20 ms, which is the server's window made visible.
        let sampler = {
            let raw = raw.clone();
            tokio::spawn(async move {
                let t0 = Instant::now();
                let mut last = raw.stats().udp_rx.datagrams;
                let mut out = Vec::new();
                for _ in 0..200 {
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    let now = raw.stats().udp_rx.datagrams;
                    out.push((t0.elapsed().as_millis(), now - last));
                    last = now;
                }
                out
            })
        };
        let t = Instant::now();
        let n = post(
            send.clone(),
            sqex_proto::blob_store::GetChunk { blob, index: 0 }.encode(),
        )
        .await;
        let took = t.elapsed();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        sampler.abort();
        println!("one chunk: {n} bytes in {took:?}");
        // (sampler output is lost on abort; sample inline instead)
        let t0 = Instant::now();
        let mut last = raw.stats().udp_rx.datagrams;
        let fetch = tokio::spawn({
            let send = send.clone();
            async move {
                post(
                    send,
                    sqex_proto::blob_store::GetChunk { blob, index: 1 }.encode(),
                )
                .await
            }
        });
        let mut line = String::new();
        while !fetch.is_finished() {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            let now = raw.stats().udp_rx.datagrams;
            line.push_str(&format!("{}:{} ", t0.elapsed().as_millis(), now - last));
            last = now;
        }
        println!("packets per 20ms: {line}");
        print("after traced chunk", &raw);
        return;
    }
    let t = Instant::now();
    let mut total = 0;
    for index in 0..chunks {
        total += post(
            send.clone(),
            sqex_proto::blob_store::GetChunk { blob, index }.encode(),
        )
        .await;
    }
    println!(
        "{chunks} chunks one at a time: {total} bytes in {:?} = {:.0} KB/s",
        t.elapsed(),
        total as f64 / t.elapsed().as_secs_f64() / 1000.0
    );
    print("after sequential fetch", &raw);
    use futures::stream::StreamExt;
    let t = Instant::now();
    let total: usize = futures::stream::iter(0..chunks)
        .map(|index| {
            post(
                send.clone(),
                sqex_proto::blob_store::GetChunk { blob, index }.encode(),
            )
        })
        .buffered(8)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .sum();
    println!(
        "{chunks} chunks 8 in flight: {total} bytes in {:?} = {:.0} KB/s",
        t.elapsed(),
        total as f64 / t.elapsed().as_secs_f64() / 1000.0
    );
    print("after parallel fetch", &raw);

    let (code, body) = begin(
        send.clone(),
        "/channel/mine".into(),
        sqex_proto::channel::Mine { offset: 0 }.encode(),
    )
    .await;
    assert_eq!(code, 200);
    let channel = sqex_proto::channel::Mines::decode(&body).unwrap().channels[0].channel;
    let (code, body) = begin(
        send.clone(),
        "/blob/begin".into(),
        sqex_proto::blob_store::Begin {
            channel,
            size: 17 * 256 * 1024,
            chunks: 17,
            expires_after: 0,
        }
        .encode(),
    )
    .await;
    assert_eq!(code, 200);
    let upload = sqex_proto::blob_store::Begun::decode(&body).unwrap().upload;
    let t = Instant::now();
    let total: usize = futures::stream::iter(0..17u32)
        .map(|index| {
            begin(
                send.clone(),
                "/blob/put".into(),
                sqex_proto::blob_store::PutChunk {
                    upload,
                    index,
                    sealed: vec![index as u8; 256 * 1024],
                }
                .encode(),
            )
        })
        .buffered(8)
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .map(|(code, _)| {
            assert_eq!(code, 200);
            256 * 1024
        })
        .sum();
    println!(
        "put 17 chunks 8 in flight: {total} bytes in {:?} = {:.0} KB/s",
        t.elapsed(),
        total as f64 / t.elapsed().as_secs_f64() / 1000.0
    );
    print("after upload", &raw);
    let _ = begin(
        send.clone(),
        "/blob/abort".into(),
        sqex_proto::blob_store::ByUpload { upload }.encode(sqex_proto::blob_store::TYPE_ABORT),
    )
    .await;
}
