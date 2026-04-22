use erpc_rust::{
    auxiliary::{MessageInfo, MessageType},
    codec::{BasicCodec, BasicCodecFactory, Codec},
    server::blocking::{
        BlockingBaseService, BlockingFunctionHandler, BlockingServer, BlockingServerBuilder,
    },
    transport::blocking_memory::BlockingMemoryTransport,
    BlockingClientManager,
};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

fn echo_service() -> BlockingBaseService {
    let mut service = BlockingBaseService::new(1);
    service.add_method(
        1,
        BlockingFunctionHandler::new(|sequence, codec| {
            let input = codec.read_string()?;
            let reply_info = MessageInfo::new(MessageType::Reply, 1, 1, sequence);
            codec.start_write_message(&reply_info)?;
            codec.write_string(&format!("Echo: {}", input))?;
            Ok(())
        }),
    );
    service
}

#[test]
fn blocking_server_poll_processes_one_request() {
    let (client_transport, server_transport) = BlockingMemoryTransport::new();
    let codec_factory = BasicCodecFactory::new();
    let mut client = BlockingClientManager::new(client_transport, codec_factory.clone());
    let mut server = BlockingServerBuilder::new()
        .transport(server_transport)
        .codec_factory(codec_factory)
        .service(Arc::new(echo_service()))
        .build()
        .unwrap();

    let client_thread = thread::spawn(move || {
        let mut request_codec = BasicCodec::new();
        request_codec.write_string("poll").unwrap();
        client
            .perform_request(1, 1, false, request_codec.as_bytes().to_vec())
            .unwrap()
    });

    let deadline = Instant::now() + Duration::from_secs(1);
    while !client_thread.is_finished() {
        server.poll().unwrap();
        assert!(Instant::now() < deadline, "server poll timed out");
    }

    let response_data = client_thread.join().unwrap();
    let mut response_codec = BasicCodec::from_data(response_data);
    assert_eq!(response_codec.read_string().unwrap(), "Echo: poll");
}

#[test]
fn blocking_server_run_processes_requests_until_stopped() {
    let (client_transport, server_transport) = BlockingMemoryTransport::new();
    let codec_factory = BasicCodecFactory::new();
    let running = Arc::new(AtomicBool::new(true));
    let server_running = Arc::clone(&running);

    let mut server = BlockingServerBuilder::new()
        .transport(server_transport)
        .codec_factory(codec_factory.clone())
        .service(Arc::new(echo_service()))
        .build()
        .unwrap();

    let server_thread =
        thread::spawn(move || server.run_while(|| server_running.load(Ordering::Acquire)));

    let mut client = BlockingClientManager::new(client_transport, codec_factory);
    for message in ["run-one", "run-two"] {
        let mut request_codec = BasicCodec::new();
        request_codec.write_string(message).unwrap();
        let response_data = client
            .perform_request(1, 1, false, request_codec.as_bytes().to_vec())
            .unwrap();
        let mut response_codec = BasicCodec::from_data(response_data);
        assert_eq!(
            response_codec.read_string().unwrap(),
            format!("Echo: {}", message)
        );
    }

    running.store(false, Ordering::Release);
    client.close().unwrap();
    server_thread.join().unwrap().unwrap();
}
