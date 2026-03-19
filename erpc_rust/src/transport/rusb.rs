use async_trait::async_trait;
use futures::{
    channel::oneshot,
    future::{select, Either},
    pin_mut,
};
use futures_timer::Delay;
use rusb::{Direction, TransferType, UsbContext};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use crate::{error::TransportError, ErpcResult};

use super::FramedTransport;

const MAX_CHUNK_SIZE: usize = 512;
const VENDOR_SPECIFIC_CLASS: u8 = 0xff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbDirection {
    In,
    Out,
}

impl UsbDirection {
    fn from_rusb(direction: Direction) -> Self {
        match direction {
            Direction::In => Self::In,
            Direction::Out => Self::Out,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsbTransferType {
    Bulk,
    Other,
}

impl UsbTransferType {
    fn from_rusb(transfer_type: TransferType) -> Self {
        match transfer_type {
            TransferType::Bulk => Self::Bulk,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointAddress {
    pub address: u8,
    pub direction: UsbDirection,
    pub transfer_type: UsbTransferType,
}

impl EndpointAddress {
    pub fn bulk_in(address: u8) -> Self {
        Self {
            address,
            direction: UsbDirection::In,
            transfer_type: UsbTransferType::Bulk,
        }
    }

    pub fn bulk_out(address: u8) -> Self {
        Self {
            address,
            direction: UsbDirection::Out,
            transfer_type: UsbTransferType::Bulk,
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
                endpoint.transfer_type == UsbTransferType::Bulk
                    && endpoint.direction == UsbDirection::In
            })
            .map(|endpoint| endpoint.address);
        let endpoint_out = candidate
            .endpoints
            .iter()
            .find(|endpoint| {
                endpoint.transfer_type == UsbTransferType::Bulk
                    && endpoint.direction == UsbDirection::Out
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

enum WorkerCommand {
    Send {
        data: Vec<u8>,
        timeout: Duration,
        reply: oneshot::Sender<ErpcResult<()>>,
    },
    Receive {
        length: usize,
        timeout: Duration,
        reply: oneshot::Sender<ErpcResult<Vec<u8>>>,
    },
    Close {
        reply: oneshot::Sender<ErpcResult<()>>,
    },
}

struct WorkerState {
    handle: rusb::DeviceHandle<rusb::Context>,
    endpoint_out: u8,
    endpoint_in: u8,
    interface_number: u8,
    read_buffer: Vec<u8>,
}

impl WorkerState {
    fn send(&mut self, data: &[u8], timeout: Duration) -> ErpcResult<()> {
        let start_time = Instant::now();

        for chunk in data.chunks(MAX_CHUNK_SIZE) {
            let remaining_timeout = timeout.saturating_sub(start_time.elapsed());
            if remaining_timeout.is_zero() {
                return Err(TransportError::Timeout.into());
            }

            let written = self
                .handle
                .write_bulk(self.endpoint_out, chunk, remaining_timeout)
                .map_err(|e| TransportError::SendFailed(e.to_string()))?;

            if written != chunk.len() {
                return Err(
                    TransportError::SendFailed("partial USB bulk write".to_string()).into(),
                );
            }
        }

        Ok(())
    }

    fn receive(&mut self, length: usize, timeout: Duration) -> ErpcResult<Vec<u8>> {
        if self.read_buffer.len() < length {
            let start_time = Instant::now();

            while self.read_buffer.len() < length {
                let remaining_timeout = timeout.saturating_sub(start_time.elapsed());
                if remaining_timeout.is_zero() {
                    return Err(TransportError::Timeout.into());
                }

                let mut temp_buf = [0u8; MAX_CHUNK_SIZE];
                let read_count = self
                    .handle
                    .read_bulk(self.endpoint_in, &mut temp_buf, remaining_timeout)
                    .map_err(|e| TransportError::ReceiveFailed(e.to_string()))?;

                if read_count > 0 {
                    self.read_buffer.extend_from_slice(&temp_buf[..read_count]);
                }

                if read_count < MAX_CHUNK_SIZE {
                    break;
                }
            }
        }

        let available_data = self.read_buffer.len().min(length);
        Ok(self.read_buffer.drain(..available_data).collect())
    }

    fn close(mut self) -> ErpcResult<()> {
        self.handle
            .release_interface(self.interface_number)
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }
}

fn spawn_worker(state: WorkerState) -> mpsc::Sender<WorkerCommand> {
    let (tx, rx) = mpsc::channel::<WorkerCommand>();

    thread::spawn(move || {
        let mut state = state;

        while let Ok(command) = rx.recv() {
            match command {
                WorkerCommand::Send {
                    data,
                    timeout,
                    reply,
                } => {
                    let _ = reply.send(state.send(&data, timeout));
                }
                WorkerCommand::Receive {
                    length,
                    timeout,
                    reply,
                } => {
                    let _ = reply.send(state.receive(length, timeout));
                }
                WorkerCommand::Close { reply } => {
                    let result = state.close();
                    let _ = reply.send(result);
                    return;
                }
            }
        }

        let _ = state.close();
    });

    tx
}

/// USB transport kept under the historical `rusb` module name for API compatibility.
///
/// Blocking USB I/O is isolated inside a worker thread so the async API does not block the caller's executor thread.
pub struct RusbTransport {
    worker_tx: Option<mpsc::Sender<WorkerCommand>>,
    timeout: Duration,
    connected: bool,
}

impl RusbTransport {
    pub async fn connect(vendor_id: u16, product_id: u16) -> ErpcResult<Self> {
        let context =
            rusb::Context::new().map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        let devices = context
            .devices()
            .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

        for device in devices.iter() {
            let descriptor = device
                .device_descriptor()
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            if descriptor.vendor_id() != vendor_id || descriptor.product_id() != product_id {
                continue;
            }

            let config = device
                .config_descriptor(0)
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
            let candidates = config
                .interfaces()
                .flat_map(|interface| interface.descriptors())
                .map(|descriptor| InterfaceCandidate {
                    interface_number: descriptor.interface_number(),
                    alternate_setting: descriptor.setting_number(),
                    class_code: descriptor.class_code(),
                    endpoints: descriptor
                        .endpoint_descriptors()
                        .map(|endpoint| EndpointAddress {
                            address: endpoint.address(),
                            direction: UsbDirection::from_rusb(endpoint.direction()),
                            transfer_type: UsbTransferType::from_rusb(endpoint.transfer_type()),
                        })
                        .collect(),
                })
                .collect::<Vec<_>>();

            let selected = select_interface_and_endpoints(&candidates)
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            let handle = device
                .open()
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            #[cfg(not(target_os = "windows"))]
            handle
                .set_auto_detach_kernel_driver(true)
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
            #[cfg(target_os = "windows")]
            {
                if let Err(e) = handle.set_auto_detach_kernel_driver(true) {
                    if e != rusb::Error::NotSupported {
                        return Err(TransportError::ConnectionFailed(e.to_string()).into());
                    }
                }
            }

            handle
                .claim_interface(selected.interface_number)
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

            if selected.alternate_setting != 0 {
                handle
                    .set_alternate_setting(selected.interface_number, selected.alternate_setting)
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
            }

            let worker_tx = spawn_worker(WorkerState {
                handle,
                endpoint_out: selected.endpoint_out,
                endpoint_in: selected.endpoint_in,
                interface_number: selected.interface_number,
                read_buffer: Vec::new(),
            });

            return Ok(Self {
                worker_tx: Some(worker_tx),
                timeout: Duration::from_millis(600),
                connected: true,
            });
        }

        Err(TransportError::ConnectionFailed("Device not found".to_string()).into())
    }

    fn worker_tx(&self) -> ErpcResult<mpsc::Sender<WorkerCommand>> {
        self.worker_tx
            .as_ref()
            .cloned()
            .ok_or_else(|| TransportError::Closed.into())
    }
}

async fn await_reply<T>(receiver: oneshot::Receiver<ErpcResult<T>>, timeout: Duration) -> ErpcResult<T> {
    let reply = receiver;
    let delay = Delay::new(timeout);
    pin_mut!(reply);
    pin_mut!(delay);

    match select(reply, delay).await {
        Either::Left((result, _)) => {
            result.map_err(|_| crate::ErpcError::from(TransportError::Closed))?
        }
        Either::Right((_, _)) => Err(TransportError::Timeout.into()),
    }
}

#[async_trait]
impl FramedTransport for RusbTransport {
    async fn base_send(&mut self, data: &[u8]) -> ErpcResult<()> {
        if !self.connected {
            return Err(TransportError::Closed.into());
        }

        let worker_tx = self.worker_tx()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        worker_tx
            .send(WorkerCommand::Send {
                data: data.to_vec(),
                timeout: self.timeout,
                reply: reply_tx,
            })
            .map_err(|_| TransportError::Closed)?;

        await_reply(reply_rx, self.timeout + self.timeout).await
    }

    async fn base_receive(&mut self, length: usize) -> ErpcResult<Vec<u8>> {
        if !self.connected {
            return Err(TransportError::Closed.into());
        }

        let worker_tx = self.worker_tx()?;
        let (reply_tx, reply_rx) = oneshot::channel();
        worker_tx
            .send(WorkerCommand::Receive {
                length,
                timeout: self.timeout,
                reply: reply_tx,
            })
            .map_err(|_| TransportError::Closed)?;

        await_reply(reply_rx, self.timeout + self.timeout).await
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn close(&mut self) -> ErpcResult<()> {
        if !self.connected {
            return Ok(());
        }

        self.connected = false;

        if let Some(worker_tx) = self.worker_tx.take() {
            let (reply_tx, reply_rx) = oneshot::channel();
            worker_tx
                .send(WorkerCommand::Close { reply: reply_tx })
                .map_err(|_| TransportError::Closed)?;
            await_reply(reply_rx, self.timeout + self.timeout).await?;
        }

        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}
