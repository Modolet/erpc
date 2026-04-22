//! Blocking client implementation for eRPC.

use crate::auxiliary::{MessageInfo, MessageType, RequestContext};
use crate::codec::{Codec, CodecFactory};
use crate::error::{ErpcResult, RequestError};
use crate::transport::BlockingTransport;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// Blocking client manager for synchronous RPC calls.
pub struct BlockingClientManager<T, F>
where
    T: BlockingTransport,
    F: CodecFactory,
{
    transport: T,
    codec_factory: F,
    sequence_counter: Arc<AtomicU32>,
}

impl<T, F> BlockingClientManager<T, F>
where
    T: BlockingTransport,
    F: CodecFactory,
{
    /// Create new blocking client manager.
    pub fn new(transport: T, codec_factory: F) -> Self {
        Self {
            transport,
            codec_factory,
            sequence_counter: Arc::new(AtomicU32::new(0)),
        }
    }

    fn next_sequence(&self) -> u32 {
        self.sequence_counter.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Create a new request context.
    pub fn create_request(&self, is_oneway: bool) -> RequestContext {
        let sequence = self.next_sequence();
        RequestContext::new(sequence, is_oneway)
    }

    /// Create a new request context with service ID.
    pub fn create_request_with_service(&self, service_id: u32, is_oneway: bool) -> RequestContext {
        let sequence = self.next_sequence();
        RequestContext::with_service(sequence, Some(service_id), is_oneway)
    }

    /// Perform a request-response call for generated code.
    pub fn perform_request(
        &mut self,
        service_id: u8,
        method_id: u8,
        is_oneway: bool,
        request_data: Vec<u8>,
    ) -> ErpcResult<Vec<u8>> {
        let sequence = self.next_sequence();
        let message_type = if is_oneway {
            MessageType::Oneway
        } else {
            MessageType::Invocation
        };
        let message_info = MessageInfo::new(message_type, service_id, method_id, sequence);

        let mut codec = self.codec_factory.create();
        codec.start_write_message(&message_info)?;
        codec.write_bytes(&request_data)?;
        self.transport.send(codec.as_bytes())?;

        if is_oneway {
            return Ok(Vec::new());
        }

        let response_data = self.transport.receive()?;
        let mut response_codec = self.codec_factory.create_from_data(response_data);
        let response_info = response_codec.start_read_message()?;

        if response_info.message_type != MessageType::Reply {
            return Err(RequestError::InvalidMessageType.into());
        }

        if response_info.sequence != sequence {
            return Err(RequestError::UnexpectedSequence {
                expected: sequence,
                actual: response_info.sequence,
            }
            .into());
        }

        response_codec.get_remaining_bytes()
    }

    /// Send raw data and optionally wait for a response.
    pub fn send_raw_request(
        &mut self,
        request_data: &[u8],
        is_oneway: bool,
    ) -> ErpcResult<Vec<u8>> {
        let _context = self.create_request(is_oneway);
        self.transport.send(request_data)?;
        if is_oneway {
            Ok(Vec::new())
        } else {
            self.transport.receive()
        }
    }

    /// Send a request without waiting for response.
    pub fn send_request(&mut self, request_data: &[u8]) -> ErpcResult<()> {
        self.send_raw_request(request_data, true)?;
        Ok(())
    }

    /// Receive a response for a previous request.
    pub fn receive_response(&mut self) -> ErpcResult<Vec<u8>> {
        self.transport.receive()
    }

    /// Get the codec factory.
    pub fn codec_factory(&self) -> &F {
        &self.codec_factory
    }

    /// Check if client is connected.
    pub fn is_connected(&self) -> bool {
        self.transport.is_connected()
    }

    /// Close the client connection.
    pub fn close(&mut self) -> ErpcResult<()> {
        self.transport.close()
    }
}
