//! Blocking in-memory transport for low-latency tests and local communication.

use crate::error::{ErpcResult, TransportError};
use crate::transport::BlockingFramedTransport;
use std::collections::VecDeque;
use std::hint::spin_loop;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct BlockingMemoryChannel {
    a_to_b: Arc<Mutex<VecDeque<Vec<u8>>>>,
    b_to_a: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl BlockingMemoryChannel {
    fn create_pair() -> (BlockingMemoryTransport, BlockingMemoryTransport) {
        let channel = Self {
            a_to_b: Arc::new(Mutex::new(VecDeque::new())),
            b_to_a: Arc::new(Mutex::new(VecDeque::new())),
        };

        let transport_a = BlockingMemoryTransport {
            send_queue: Arc::clone(&channel.a_to_b),
            recv_queue: Arc::clone(&channel.b_to_a),
            timeout: Duration::from_millis(10),
            connected: true,
        };

        let transport_b = BlockingMemoryTransport {
            send_queue: channel.b_to_a,
            recv_queue: channel.a_to_b,
            timeout: Duration::from_millis(10),
            connected: true,
        };

        (transport_a, transport_b)
    }
}

/// Blocking in-memory transport that spins while polling for incoming bytes.
pub struct BlockingMemoryTransport {
    send_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    recv_queue: Arc<Mutex<VecDeque<Vec<u8>>>>,
    timeout: Duration,
    connected: bool,
}

impl BlockingMemoryTransport {
    /// Create paired blocking memory transports.
    pub fn new() -> (Self, Self) {
        BlockingMemoryChannel::create_pair()
    }

    /// Create paired blocking memory transports.
    pub fn pair() -> (Self, Self) {
        Self::new()
    }
}

impl BlockingFramedTransport for BlockingMemoryTransport {
    fn base_send(&mut self, data: &[u8]) -> ErpcResult<()> {
        if !self.connected {
            return Err(TransportError::Closed.into());
        }

        let mut queue = self
            .send_queue
            .lock()
            .map_err(|_| TransportError::SendFailed("memory queue lock poisoned".to_string()))?;
        queue.push_back(data.to_vec());
        Ok(())
    }

    fn base_receive(&mut self, length: usize) -> ErpcResult<Vec<u8>> {
        if !self.connected {
            return Err(TransportError::Closed.into());
        }

        let start = Instant::now();
        let mut buffer = Vec::with_capacity(length);

        while buffer.len() < length {
            {
                let mut queue = self.recv_queue.lock().map_err(|_| {
                    TransportError::ReceiveFailed("memory queue lock poisoned".to_string())
                })?;
                if let Some(data) = queue.pop_front() {
                    buffer.extend_from_slice(&data);
                }
            }

            if buffer.len() >= length {
                break;
            }

            if start.elapsed() > self.timeout {
                return Err(TransportError::Timeout.into());
            }

            spin_loop();
        }

        if buffer.len() > length {
            let excess = buffer.split_off(length);
            let mut queue = self.recv_queue.lock().map_err(|_| {
                TransportError::ReceiveFailed("memory queue lock poisoned".to_string())
            })?;
            queue.push_front(excess);
        }

        Ok(buffer)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn close(&mut self) -> ErpcResult<()> {
        self.connected = false;
        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}
