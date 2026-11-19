# eRPC Rust

A Rust implementation of the eRPC (Embedded RPC) protocol, providing efficient client and server functionality with primary focus on TCP transport.

## Features

- **TCP-First Design**: Primary focus on TCP transport for reliable network communication
- **Async/Await Support**: Built on Tokio for high-performance async I/O
- **Codec System**: Binary protocol with little-endian encoding using BasicCodec
- **Message Types**: Support for invocation, oneway, reply, and notification messages
- **Error Handling**: Comprehensive error types with detailed error information
- **Thread Safe**: All components are designed for concurrent use
- **Additional Transports**: Memory transport for testing, Serial transport available as optional feature

## Complete Workflow: From IDL to Implementation

### Step 1: Define Your Service (IDL)

Create an `.erpc` file defining your service interface:

```idl
// temp_alarm.erpc
program TempAlarm

type SensorAddress = uint8

struct SensorInfo {
    SensorAddress address
    float readInterval
}

interface Temp {
    add_sensor(SensorAddress address) -> bool
    remove_sensor(SensorAddress address) -> bool
    set_interval(SensorAddress address, float interval) -> bool
    read_one_sensor(SensorAddress address) -> float
}

interface TempAsync {
    oneway sensor_reading(SensorAddress addr, float temp)
}
```

### Step 2: Generate Rust Code

Use the eRPC code generator to create Rust bindings:

```bash
# Navigate to your eRPC installation
cd /path/to/erpc

# Generate Rust code from your IDL file
./Release/Linux/erpcgen/erpcgen -g rust -o ./generated/ ./temp_alarm.erpc

# Alternative: If you built eRPC locally
make -C erpcgen  # Build the generator first
./erpcgen/bin/erpcgen -g rust -o ./generated/ ./temp_alarm.erpc

# This creates:
# - generated/temp_alarm.rs (service definitions, client/server traits)
```

**Generated Code Structure:**
- **Service IDs**: Enums defining unique service identifiers
- **Method IDs**: Enums for each service's method identifiers  
- **Data Types**: Structs, enums, and type aliases from your IDL
- **Client Traits**: Async trait definitions for service clients
- **Client Structs**: Concrete client implementations
- **Server Traits**: Async trait definitions you implement for services
- **Server Structs**: Server wrappers that handle the eRPC protocol

### Step 3: Setup Your Rust Project

Add dependencies to your `Cargo.toml`:

```toml
[dependencies]
erpc_rust = "0.3"
tokio = { version = "1.0", features = ["full"] }
async-trait = "0.1"
serde = { version = "1.0", features = ["derive"] }
```

Include the generated code in your project:

```rust
// src/main.rs
mod temp_alarm; // Include the generated temp_alarm.rs

use temp_alarm::temp_server::{Temp, TempClient, TempServer};
use temp_alarm::temp_async_server::{TempAsync, TempAsyncClient, TempAsyncServer};
```

### Step 4: Implement Server

Create your service implementation:

```rust
// src/server_impl.rs
use crate::temp_alarm::temp_server::{Temp, TempServer};
use crate::temp_alarm::temp_async_server::{TempAsync, TempAsyncServer};
use async_trait::async_trait;
use erpc_rust::{
    codec::BasicCodecFactory,
    server::{MultiTransportServerBuilder, Server},
};

#[derive(Clone)]
struct TempServiceImpl {
    // Your service state here
}

#[async_trait]
impl Temp for TempServiceImpl {
    async fn add_sensor(&self, address: u8) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        println!("Adding sensor with address: {}", address);
        // Your implementation logic here
        Ok(true)
    }

    async fn remove_sensor(&self, address: u8) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        println!("Removing sensor with address: {}", address);
        Ok(true)
    }

    async fn set_interval(&self, address: u8, interval: f32) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
        println!("Setting interval for sensor {}: {}", address, interval);
        Ok(true)
    }

    async fn read_one_sensor(&self, address: u8) -> Result<f32, Box<dyn std::error::Error + Send + Sync>> {
        println!("Reading sensor {}", address);
        Ok(23.5) // Mock temperature reading
    }
}

#[async_trait]
impl TempAsync for TempServiceImpl {
    async fn sensor_reading(&self, addr: u8, temp: f32) {
        println!("Received sensor reading: addr={}, temp={}", addr, temp);
    }
}

pub async fn run_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("🚀 Starting eRPC Temperature Server...");

    // Create service implementations
    let temp_service = TempServiceImpl {};
    let temp_async_service = TempServiceImpl {};

    // Build and start server
    let mut server = MultiTransportServerBuilder::new()
        .add_tcp_transport("127.0.0.1:40000")
        .await?
        .codec_factory(BasicCodecFactory::new())
        .add_service(TempServer::new(temp_service))
        .add_service(TempAsyncServer::new(temp_async_service))
        .build();

    println!("✅ Server listening on 127.0.0.1:40000");
    server.run().await?;
    Ok(())
}
```

#### Server Wiring Patterns

The eRPC Rust server supports multiple wiring patterns for different deployment scenarios:

**Single Service, Single Transport:**
```rust
use erpc_rust::{
    codec::BasicCodecFactory,
    server::{Server, SingleTransportServerBuilder},
    transport::TcpTransport,
};

// Simple single service setup
let transport = TcpTransport::bind("127.0.0.1:40000").await?;
let mut server = SingleTransportServerBuilder::new(transport)
    .codec_factory(BasicCodecFactory::new())
    .add_service(TempServer::new(TempServiceImpl::new()))
    .build();

server.run().await?;
```

**Multiple Services, Single Transport:**
```rust
// Multiple services on same transport
let mut server = MultiTransportServerBuilder::new()
    .add_tcp_transport("127.0.0.1:40000").await?
    .codec_factory(BasicCodecFactory::new())
    .add_service(TempServer::new(temp_service))
    .add_service(TempAsyncServer::new(temp_async_service))
    .add_service(OtherServer::new(other_service))  // Add more services
    .build();
```

**Multiple Services, Multiple Transports:**
```rust
// Services across different transports
let mut server = MultiTransportServerBuilder::new()
    .add_tcp_transport("127.0.0.1:40000").await?      // TCP for main services
    .add_tcp_transport("127.0.0.1:40001").await?      // TCP for admin services
    .codec_factory(BasicCodecFactory::new())
    .add_service(TempServer::new(temp_service))
    .add_service(TempAsyncServer::new(temp_async_service))
    .build();
```

**Server with Serial Transport:**
```rust
// Mixed TCP and Serial transports
let mut server = MultiTransportServerBuilder::new()
    .add_tcp_transport("127.0.0.1:40000").await?
    .add_serial_transport("/dev/ttyUSB0", 115200).await?
    .codec_factory(BasicCodecFactory::new())
    .add_service(TempServer::new(temp_service))
    .build();
```

**Service Discovery and Registration:**
```rust
// Advanced server setup with service registration
pub async fn setup_production_server() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut builder = MultiTransportServerBuilder::new();
    
    // Add transports
    builder = builder
        .add_tcp_transport("0.0.0.0:40000").await?  // Listen on all interfaces
        .codec_factory(BasicCodecFactory::new());
    
    // Register services dynamically
    let services = vec![
        Box::new(TempServer::new(TempServiceImpl::new())) as Box<dyn Service + Send + Sync>,
        Box::new(TempAsyncServer::new(TempAsyncServiceImpl::new())),
        // Add more services as needed
    ];
    
    for service in services {
        builder = builder.add_service(service);
    }
    
    let mut server = builder.build();
    
    // Graceful shutdown handling
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                eprintln!("Server error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Received Ctrl+C, shutting down gracefully...");
        }
    }
    
    Ok(())
}
```

### Step 5: Implement Client

Create one connection, then construct a client for each service. Each client owns
a handle to the connection and can be moved into a task or cloned independently.

```rust
use crate::temp_alarm::temp_server::TempClient;
use crate::temp_alarm::temp_async_server::TempAsyncClient;
use erpc_rust::{codec::BasicCodecFactory, Connection, TcpTransport};

pub async fn run_client() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let transport = TcpTransport::connect("127.0.0.1:40000").await?;
    let connection = Connection::new(transport, BasicCodecFactory::new());
    let temp = TempClient::new(&connection);
    let notifications = TempAsyncClient::new(&connection);

    let reading = tokio::spawn(async move { temp.read_one_sensor(1).await });
    notifications.sensor_reading(1, 25.3).await?;
    println!("Temperature: {}", reading.await??);
    connection.close().await?;
    Ok(())
}
```

Run the self-contained two-task example with `cargo run --example shared_connection`.
Its IDL and generated bindings are in `examples/shared_connection` inside the crate.
To regenerate, run `erpcgen -g rust -o examples/shared_connection examples/shared_connection/services.erpc`
from the crate directory, then run
`rustfmt --edition 2021 examples/shared_connection/main.rs`.

The connection serializes complete exchanges. Dropping one client releases its
handle. Closing the connection affects all clients. Cancelling a queued call is
safe; cancelling an in-flight exchange or receiving a request error retires the
connection. Open a new transport to reconnect.

When migrating async bindings, regenerate them with the matching erpcgen.
Existing manager builders can be used with `Connection::from(manager)`.
Blocking clients continue to take a mutable blocking manager.

### Step 6: Main Application

Wire everything together:

```rust
// src/main.rs
use std::env;

mod temp_alarm;    // Generated code
mod server_impl;   // Your server implementation
mod client_impl;   // Your client implementation

use client_impl::run_client;
use server_impl::run_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        println!("Usage: {} <server|client>", args[0]);
        return Ok(());
    }

    match args[1].as_str() {
        "server" => run_server().await,
        "client" => run_client().await.map_err(|e| e.into()),
        _ => {
            println!("Invalid argument. Use 'server' or 'client'");
            Ok(())
        }
    }
}
```

### Step 7: Run Your Application

```bash
# Build the project
cargo build

# Run server in one terminal
cargo run server

# Run client in another terminal
cargo run client
```

## Serial Transport Support

Enable serial transport with the `serial` feature:

```toml
[dependencies]
erpc_rust = { version = "0.3", features = ["serial"] }
```

### Serial Connection Examples

```rust
use erpc_rust::client::{ClientManager, CodecConfig};

// Serial with default baud rate (115200)
let client = ClientManager::builder()
    .serial_connection("/dev/ttyUSB0")
    .codec(CodecConfig::Basic)
    .connect().await?;

// Serial with custom baud rate
let client = ClientManager::builder()
    .serial_connection_with_baud("/dev/ttyACM0", 9600)
    .codec(CodecConfig::Basic)
    .connect().await?;
```

## Transport Options

### TCP Transport (Primary Focus)
The main transport implementation providing:
- Reliable network communication
- Framed message support with CRC validation
- Connection management and error handling
- Compatible with eRPC TCP implementations in other languages

### Additional Transports
- **Memory Transport**: For testing and in-process communication
- **Serial Transport**: Available with `serial` feature for embedded applications

## Features

### Default Features
- `tcp`: TCP transport (always enabled)
- `serial`: Serial port transport (optional, requires system dependencies)

### Building with specific features:
```bash
# TCP only (default)
cargo build --no-default-features --features tcp

# TCP + Serial
cargo build --features "tcp,serial"
```
