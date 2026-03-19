use async_trait::async_trait;
use futures::{future::poll_fn, pin_mut, Future};
use futures_timer::Delay;
use nusb::{
    descriptors::TransferType,
    transfer::{Buffer, Bulk, BulkOrInterrupt, Direction, EndpointDirection, In, Out},
    Endpoint,
    MaybeFuture,
};
use std::{
    task::Poll,
    time::{Duration, Instant},
};

use crate::{error::TransportError, ErpcResult};

use super::FramedTransport;

const MAX_CHUNK_SIZE: usize = 512;
const VENDOR_SPECIFIC_CLASS: u8 = 0xff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointAddress {
    pub address: u8,
    pub direction: Direction,
    pub transfer_type: TransferType,
}

impl EndpointAddress {
    pub fn bulk_in(address: u8) -> Self {
        Self {
            address,
            direction: Direction::In,
            transfer_type: TransferType::Bulk,
        }
    }

    pub fn bulk_out(address: u8) -> Self {
        Self {
            address,
            direction: Direction::Out,
            transfer_type: TransferType::Bulk,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCandidate {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub class_code: u8,
    pub endpoints: Vec<EndpointAddress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectedInterface {
    pub interface_number: u8,
    pub alternate_setting: u8,
    pub endpoint_in: u8,
    pub endpoint_out: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionError {
    VendorInterfaceNotFound,
    MissingBulkPair,
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SelectionError::VendorInterfaceNotFound => write!(f, "Vendor interface not found"),
            SelectionError::MissingBulkPair => {
                write!(f, "Missing bulk IN/OUT endpoints for vendor interface")
            }
        }
    }
}

pub fn select_interface_and_endpoints(
    candidates: &[InterfaceCandidate],
) -> Result<SelectedInterface, SelectionError> {
    let mut saw_vendor_interface = false;

    for candidate in candidates {
        if candidate.class_code != VENDOR_SPECIFIC_CLASS {
            continue;
        }

        saw_vendor_interface = true;

        let endpoint_in = candidate
            .endpoints
            .iter()
            .find(|endpoint| {
                endpoint.transfer_type == TransferType::Bulk && endpoint.direction == Direction::In
            })
            .map(|endpoint| endpoint.address);
        let endpoint_out = candidate
            .endpoints
            .iter()
            .find(|endpoint| {
                endpoint.transfer_type == TransferType::Bulk && endpoint.direction == Direction::Out
            })
            .map(|endpoint| endpoint.address);

        if let (Some(endpoint_in), Some(endpoint_out)) = (endpoint_in, endpoint_out) {
            return Ok(SelectedInterface {
                interface_number: candidate.interface_number,
                alternate_setting: candidate.alternate_setting,
                endpoint_in,
                endpoint_out,
            });
        }
    }

    if saw_vendor_interface {
        Err(SelectionError::MissingBulkPair)
    } else {
        Err(SelectionError::VendorInterfaceNotFound)
    }
}

/// USB transport kept under the historical `rusb` module name for API compatibility.
///
/// The implementation is backed by `nusb` to provide runtime-agnostic bulk transfer futures.
pub struct RusbTransport {
    interface: Option<nusb::Interface>,
    endpoint_out: Option<Endpoint<Bulk, Out>>,
    endpoint_in: Option<Endpoint<Bulk, In>>,
    timeout: Duration,
    connected: bool,
    read_buffer: Vec<u8>,
    in_packet_size: usize,
}

impl RusbTransport {
    pub async fn connect(vendor_id: u16, product_id: u16) -> ErpcResult<Self> {
        let device_info = nusb::list_devices()
            .wait()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?
            .find(|device| device.vendor_id() == vendor_id && device.product_id() == product_id)
            .ok_or_else(|| TransportError::ConnectionFailed("Device not found".to_string()))?;

        let device = device_info
            .open()
            .wait()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        let active_configuration = device
            .active_configuration()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        let candidates = active_configuration
            .interface_alt_settings()
            .map(|descriptor| InterfaceCandidate {
                interface_number: descriptor.interface_number(),
                alternate_setting: descriptor.alternate_setting(),
                class_code: descriptor.class(),
                endpoints: descriptor
                    .endpoints()
                    .map(|endpoint| EndpointAddress {
                        address: endpoint.address(),
                        direction: endpoint.direction(),
                        transfer_type: endpoint.transfer_type(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();

        let selected = select_interface_and_endpoints(&candidates)
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        let interface = device
            .detach_and_claim_interface(selected.interface_number)
            .wait()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        if selected.alternate_setting != interface.get_alt_setting() {
            interface
                .set_alt_setting(selected.alternate_setting)
                .wait()
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        }

        let endpoint_out = interface
            .endpoint::<Bulk, Out>(selected.endpoint_out)
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        let endpoint_in = interface
            .endpoint::<Bulk, In>(selected.endpoint_in)
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        let in_packet_size = endpoint_in.max_packet_size();

        Ok(Self {
            interface: Some(interface),
            endpoint_out: Some(endpoint_out),
            endpoint_in: Some(endpoint_in),
            timeout: Duration::from_millis(600),
            connected: true,
            read_buffer: Vec::new(),
            in_packet_size,
        })
    }

    fn ensure_open(&self) -> ErpcResult<()> {
        if self.connected {
            Ok(())
        } else {
            Err(TransportError::Closed.into())
        }
    }

    fn endpoint_out_mut(&mut self) -> ErpcResult<&mut Endpoint<Bulk, Out>> {
        self.endpoint_out
            .as_mut()
            .ok_or_else(|| TransportError::Closed.into())
    }

    fn endpoint_in_mut(&mut self) -> ErpcResult<&mut Endpoint<Bulk, In>> {
        self.endpoint_in
            .as_mut()
            .ok_or_else(|| TransportError::Closed.into())
    }

    fn remaining_timeout(&self, start_time: Instant) -> ErpcResult<Duration> {
        let remaining = self.timeout.saturating_sub(start_time.elapsed());
        if remaining.is_zero() {
            Err(TransportError::Timeout.into())
        } else {
            Ok(remaining)
        }
    }

    fn round_in_request_len(&self, min_len: usize) -> usize {
        let packet_size = self.in_packet_size.max(1);
        let target = min_len.max(MAX_CHUNK_SIZE).max(packet_size);
        target.div_ceil(packet_size) * packet_size
    }
}

async fn wait_for_completion<EpType, Dir>(
    endpoint: &mut Endpoint<EpType, Dir>,
    timeout: Duration,
) -> Result<nusb::transfer::Completion, TransportError>
where
    EpType: BulkOrInterrupt,
    Dir: EndpointDirection,
{
    let delay = Delay::new(timeout);
    pin_mut!(delay);
    let mut timed_out = false;

    poll_fn(|cx| {
        if let Poll::Ready(completion) = endpoint.poll_next_complete(cx) {
            return if timed_out {
                Poll::Ready(Err(TransportError::Timeout))
            } else {
                Poll::Ready(Ok(completion))
            };
        }

        if !timed_out && delay.as_mut().poll(cx).is_ready() {
            timed_out = true;
            endpoint.cancel_all();
            cx.waker().wake_by_ref();
        }

        Poll::Pending
    })
    .await
}

fn map_send_error(error: impl ToString) -> crate::ErpcError {
    TransportError::SendFailed(error.to_string()).into()
}

fn map_receive_error(error: impl ToString) -> crate::ErpcError {
    TransportError::ReceiveFailed(error.to_string()).into()
}

#[async_trait]
impl FramedTransport for RusbTransport {
    async fn base_send(&mut self, data: &[u8]) -> ErpcResult<()> {
        self.ensure_open()?;
        let start_time = Instant::now();

        for chunk in data.chunks(MAX_CHUNK_SIZE) {
            let remaining_timeout = self.remaining_timeout(start_time)?;
            let endpoint_out = self.endpoint_out_mut()?;
            endpoint_out.submit(chunk.to_vec().into());

            let completion = wait_for_completion(endpoint_out, remaining_timeout).await?;
            completion.status.map_err(map_send_error)?;
        }

        Ok(())
    }

    async fn base_receive(&mut self, length: usize) -> ErpcResult<Vec<u8>> {
        self.ensure_open()?;

        if self.read_buffer.len() < length {
            let start_time = Instant::now();

            while self.read_buffer.len() < length {
                let remaining_timeout = self.remaining_timeout(start_time)?;
                let requested_len = self.round_in_request_len(length - self.read_buffer.len());
                let endpoint_in = self.endpoint_in_mut()?;
                endpoint_in.submit(Buffer::new(requested_len));

                let completion = wait_for_completion(endpoint_in, remaining_timeout).await?;
                let status = completion.status;
                let packet = completion.buffer.into_vec();
                let packet_len = packet.len();

                if packet_len > 0 {
                    self.read_buffer.extend_from_slice(&packet);
                }

                match status {
                    Ok(()) => {
                        if packet_len < requested_len {
                            break;
                        }
                    }
                    Err(error) => {
                        if self.read_buffer.is_empty() {
                            return Err(map_receive_error(error));
                        }
                        break;
                    }
                }
            }
        }

        let available_data = self.read_buffer.len().min(length);
        let result = self.read_buffer.drain(..available_data).collect();

        Ok(result)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn close(&mut self) -> ErpcResult<()> {
        if self.connected {
            self.connected = false;
            self.endpoint_in = None;
            self.endpoint_out = None;
            self.interface = None;
            self.read_buffer.clear();
        }

        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}
