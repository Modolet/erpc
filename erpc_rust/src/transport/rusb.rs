use async_trait::async_trait;
use rusb::{Direction, TransferType, UsbContext};
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::{error::TransportError, ErpcResult};

use super::FramedTransport;

const MAX_CHUNK_SIZE: usize = 512;

pub struct RusbTransport {
    handle: Mutex<rusb::DeviceHandle<rusb::Context>>,
    timeout: Duration,
    connected: bool,
    endpoint_out: u8,
    endpoint_in: u8,
    interface_number: u8,
    read_buffer: Mutex<Vec<u8>>,
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

            if descriptor.vendor_id() == vendor_id && descriptor.product_id() == product_id {
                let handle = device
                    .open()
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
                let config = device
                    .config_descriptor(0)
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

                // 查找目标接口和端点
                let mut target_interface = None;
                let mut endpoint_in = None;
                let mut endpoint_out = None;

                for interface in config.interfaces() {
                    for descriptor in interface.descriptors() {
                        if descriptor.class_code() == rusb::constants::LIBUSB_CLASS_VENDOR_SPEC {
                            // 0xFF
                            target_interface = Some(descriptor.interface_number());

                            for endpoint in descriptor.endpoint_descriptors() {
                                if endpoint.transfer_type() == TransferType::Bulk {
                                    match endpoint.direction() {
                                        Direction::In => endpoint_in = Some(endpoint.address()),
                                        Direction::Out => endpoint_out = Some(endpoint.address()),
                                    }
                                }
                            }
                            // 找到Vendor接口后就跳出循环，不再查找其他接口
                            break;
                        }
                    }
                    if target_interface.is_some() {
                        break;
                    }
                }

                let interface_number = target_interface.ok_or_else(|| {
                    TransportError::ConnectionFailed("Vendor interface not found".into())
                })?;
                let endpoint_in = endpoint_in.ok_or_else(|| {
                    TransportError::ConnectionFailed(
                        "Missing bulk IN endpoint for vendor interface".into(),
                    )
                })?;
                let endpoint_out = endpoint_out.ok_or_else(|| {
                    TransportError::ConnectionFailed(
                        "Missing bulk OUT endpoint for vendor interface".into(),
                    )
                })?;

                // 自动解除可能存在的内核驱动占用（Windows 上该接口不受支持）
                #[cfg(not(target_os = "windows"))]
                handle
                    .set_auto_detach_kernel_driver(true)
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
                #[cfg(target_os = "windows")]
                {
                    if let Err(e) = handle.set_auto_detach_kernel_driver(true) {
                        use rusb::Error;
                        if e != Error::NotSupported {
                            return Err(TransportError::ConnectionFailed(e.to_string()).into());
                        }
                    }
                }
                // 声明对找到的Vendor接口的独占权
                handle
                    .claim_interface(interface_number)
                    .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;

                let handle = Mutex::new(handle);
                let read_buffer = Mutex::new(Vec::new());

                return Ok(Self {
                    interface_number,
                    handle,
                    endpoint_in,
                    endpoint_out,
                    connected: true,
                    timeout: Duration::from_millis(600),
                    read_buffer,
                });
            }
        }
        Err(TransportError::ConnectionFailed("Device not found".to_string()).into())
    }
}

#[async_trait]
impl FramedTransport for RusbTransport {
    async fn base_send(&mut self, data: &[u8]) -> ErpcResult<()> {
        let mut handle = self.handle.lock().await;
        let start_time = Instant::now();
        let mut total_written = 0;

        for chunk in data.chunks(MAX_CHUNK_SIZE) {
            if start_time.elapsed() >= self.timeout {
                break;
            }

            let remaining_timeout = self.timeout.saturating_sub(start_time.elapsed());
            if remaining_timeout.is_zero() {
                break;
            }

            match handle.write_bulk(self.endpoint_out, chunk, remaining_timeout) {
                Ok(written) => {
                    total_written += written;
                    if written < chunk.len() {
                        break;
                    }
                }
                Err(e) => {
                    if total_written == 0 {
                        return Err(TransportError::SendFailed(e.to_string()).into());
                    } else {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    async fn base_receive(&mut self, length: usize) -> ErpcResult<Vec<u8>> {
        let mut handle = self.handle.lock().await;
        let mut buffer = self.read_buffer.lock().await;

        // 如果缓冲区中的数据不足，尝试从设备读取更多数据
        if buffer.len() < length {
            let start_time = Instant::now();

            while buffer.len() < length {
                // 检查是否超时
                if start_time.elapsed() >= self.timeout {
                    break;
                }

                // 计算剩余超时时间
                let remaining_timeout = self.timeout.saturating_sub(start_time.elapsed());
                if remaining_timeout.is_zero() {
                    break;
                }

                // 固定使用512字节的缓冲区进行读取
                let mut temp_buf = [0u8; MAX_CHUNK_SIZE];

                match handle.read_bulk(self.endpoint_in, &mut temp_buf, remaining_timeout) {
                    Ok(read_count) => {
                        if read_count > 0 {
                            buffer.extend_from_slice(&temp_buf[..read_count]);
                        }
                        // 如果读取字节数小于请求的字节数，通常表示这是最后一个数据包
                        if read_count < MAX_CHUNK_SIZE {
                            break;
                        }
                    }
                    Err(e) => {
                        if buffer.is_empty() {
                            return Err(TransportError::ReceiveFailed(e.to_string()).into());
                        } else {
                            break;
                        }
                    }
                }
            }
        }

        // 从缓冲区中取出请求长度的数据
        let available_data = buffer.len().min(length);
        let result = buffer.drain(..available_data).collect();

        Ok(result)
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn close(&mut self) -> ErpcResult<()> {
        if self.connected {
            self.connected = false;
            self.handle
                .lock()
                .await
                .release_interface(self.interface_number)
                .map_err(|e| TransportError::ConnectionFailed(e.to_string()))?;
        };
        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) {
        self.timeout = timeout;
    }
}
