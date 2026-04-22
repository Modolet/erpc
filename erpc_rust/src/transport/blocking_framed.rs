//! Blocking framed transport base implementation with CRC validation.

use crate::{
    auxiliary::{crc16, utils},
    codec::{BasicCodec, Codec},
    error::{ErpcResult, TransportError},
    transport::BlockingTransport,
};
use std::time::Duration;

const HEADER_LEN: usize = 6;

/// Blocking framed transport for synchronous low-latency implementations.
pub trait BlockingFramedTransport: Send + Sync {
    /// Send raw data without framing.
    fn base_send(&mut self, data: &[u8]) -> ErpcResult<()>;

    /// Receive exact number of raw bytes without framing.
    fn base_receive(&mut self, length: usize) -> ErpcResult<Vec<u8>>;

    /// Check if the transport is connected.
    fn is_connected(&self) -> bool;

    /// Close the transport.
    fn close(&mut self) -> ErpcResult<()>;

    /// Set timeout for operations.
    fn set_timeout(&mut self, timeout: Duration);
}

impl<T: BlockingFramedTransport + 'static> BlockingTransport for T {
    fn send(&mut self, data: &[u8]) -> ErpcResult<()> {
        let mut codec = BasicCodec::new();

        let message_length = data.len() as u16;
        let crc_body = crc16::calculate(data);

        let length_bytes = utils::uint16_to_bytes(message_length);
        let crc_body_bytes = utils::uint16_to_bytes(crc_body);
        let crc_length = crc16::calculate(&length_bytes);
        let crc_body_crc = crc16::calculate(&crc_body_bytes);
        let crc_header = crc_length.wrapping_add(crc_body_crc);

        codec.write_uint16(crc_header)?;
        codec.write_uint16(message_length)?;
        codec.write_uint16(crc_body)?;

        let header = codec.as_bytes();
        self.base_send(header)?;
        self.base_send(data)?;
        Ok(())
    }

    fn receive(&mut self) -> ErpcResult<Vec<u8>> {
        let header_data = self.base_receive(HEADER_LEN)?;
        let mut codec = BasicCodec::from_data(header_data);

        let crc_header = codec.read_uint16()?;
        let message_length = codec.read_uint16()?;
        let crc_body = codec.read_uint16()?;

        let length_bytes = utils::uint16_to_bytes(message_length);
        let crc_body_bytes = utils::uint16_to_bytes(crc_body);
        let computed_crc_length = crc16::calculate(&length_bytes);
        let computed_crc_body_crc = crc16::calculate(&crc_body_bytes);
        let computed_crc_header = computed_crc_length.wrapping_add(computed_crc_body_crc);

        if computed_crc_header != crc_header {
            return Err(
                TransportError::ReceiveFailed("Invalid message (header) CRC".to_string()).into(),
            );
        }

        let data = self.base_receive(message_length as usize)?;
        let computed_body_crc = crc16::calculate(&data);
        if computed_body_crc != crc_body {
            return Err(
                TransportError::ReceiveFailed("Invalid message (body) CRC".to_string()).into(),
            );
        }

        Ok(data)
    }

    fn is_connected(&self) -> bool {
        BlockingFramedTransport::is_connected(self)
    }

    fn close(&mut self) -> ErpcResult<()> {
        BlockingFramedTransport::close(self)
    }

    fn set_timeout(&mut self, timeout: Duration) {
        BlockingFramedTransport::set_timeout(self, timeout)
    }
}
