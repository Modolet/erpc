mod client;
mod generated;
mod server;

use client::run_client;
use erpc_rust::{
    codec::BasicCodecFactory, server::blocking::BlockingServerBuilder,
    transport::blocking_memory::BlockingMemoryTransport, BlockingClientManager,
};
use server::{TelemetryService, LAST_HEARTBEAT};
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (client_transport, server_transport) = BlockingMemoryTransport::pair();
    let codec_factory = BasicCodecFactory::new();
    let running = Arc::new(AtomicBool::new(true));
    let server_running = Arc::clone(&running);

    let service = TelemetryService::new();
    let telemetry_server = generated::telemetry_server::TelemetryServer::new(service);
    let mut server = BlockingServerBuilder::new()
        .transport(server_transport)
        .codec_factory(codec_factory.clone())
        .service(Arc::new(telemetry_server))
        .build()?;

    let server_thread =
        thread::spawn(move || server.run_while(|| server_running.load(Ordering::Acquire)));

    let mut manager = BlockingClientManager::new(client_transport, codec_factory);
    run_client(&mut manager)?;

    running.store(false, Ordering::Release);
    manager.close()?;
    server_thread.join().expect("server thread panicked")?;

    let heartbeat = LAST_HEARTBEAT.lock().expect("heartbeat lock poisoned");
    println!("last heartbeat: {:?}", *heartbeat);
    Ok(())
}
