//! Transport layer abstraction for eRPC

use crate::error::{ErpcResult, TransportError};
use async_trait::async_trait;
use std::time::Duration;

/// Transport trait for different communication methods
#[async_trait]
pub trait Transport: Send + Sync {
    /// Send data through the transport
    async fn send(&mut self, data: &[u8]) -> ErpcResult<()>;

    /// Receive data from the transport
    async fn receive(&mut self) -> ErpcResult<Vec<u8>>;

    /// Close the transport
    async fn close(&mut self) -> ErpcResult<()>;

    /// Check if transport is connected
    fn is_connected(&self) -> bool;

    /// Set timeout for operations
    fn set_timeout(&mut self, timeout: Duration);
}

/// Blocking transport trait for low-latency synchronous communication.
pub trait BlockingTransport: Send + Sync {
    /// Send data through the transport.
    fn send(&mut self, data: &[u8]) -> ErpcResult<()>;

    /// Receive one complete framed message.
    fn receive(&mut self) -> ErpcResult<Vec<u8>>;

    /// Close the transport.
    fn close(&mut self) -> ErpcResult<()>;

    /// Check if transport is connected.
    fn is_connected(&self) -> bool;

    /// Set timeout for operations.
    fn set_timeout(&mut self, timeout: Duration);
}

/// Transport factory trait for creating transport instances
#[async_trait]
pub trait TransportFactory: Send + Sync {
    type Transport: Transport;

    /// Create a new transport instance
    async fn create(&self) -> ErpcResult<Self::Transport>;
}

pub mod blocking_framed;
pub mod blocking_memory;
pub mod framed;
pub mod memory;
pub mod rusb;
pub mod serial;
#[cfg(unix)]
pub mod socket;
pub mod tcp;

#[cfg(feature = "tcp")]
pub use tcp::TcpTransport;

#[cfg(feature = "serial")]
pub use serial::SerialTransport;

#[cfg(unix)]
pub use socket::SocketTransport;

pub use blocking_framed::BlockingFramedTransport;
pub use blocking_memory::BlockingMemoryTransport;
pub use framed::FramedTransport;
pub use memory::MemoryTransport;
