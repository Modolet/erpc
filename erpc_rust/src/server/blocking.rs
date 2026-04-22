//! Blocking server implementation for eRPC.

use crate::auxiliary::MessageType;
use crate::codec::{Codec, CodecFactory};
use crate::error::{ErpcResult, RequestError, TransportError};
use crate::transport::BlockingTransport;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Blocking server trait for handling eRPC requests.
pub trait BlockingServer {
    /// Add a service to the server.
    fn add_service(&mut self, service: Arc<dyn BlockingService>) -> ErpcResult<()>;

    /// Remove a service from the server.
    fn remove_service(&mut self, service_id: u8) -> ErpcResult<()>;

    /// Poll and process at most one incoming request.
    fn poll(&mut self) -> ErpcResult<bool>;

    /// Run until stopped or the transport disconnects.
    fn run(&mut self) -> ErpcResult<()>;

    /// Stop the server.
    fn stop(&mut self) -> ErpcResult<()>;

    /// Check if server is running.
    fn is_running(&self) -> bool;
}

/// Blocking service trait for handling method calls.
pub trait BlockingService: Send + Sync {
    /// Get service ID.
    fn service_id(&self) -> u8;

    /// Handle method invocation.
    fn handle_invocation(
        &self,
        method_id: u8,
        sequence: u32,
        codec: &mut dyn Codec,
    ) -> ErpcResult<()>;

    /// Get list of supported method IDs.
    fn supported_methods(&self) -> Vec<u8>;
}

/// Simple single-threaded blocking server implementation.
pub struct BlockingSimpleServer<T, F>
where
    T: BlockingTransport,
    F: CodecFactory,
{
    transport: T,
    codec_factory: F,
    services: HashMap<u8, Arc<dyn BlockingService>>,
    running: bool,
}

impl<T, F> BlockingSimpleServer<T, F>
where
    T: BlockingTransport + 'static,
    F: CodecFactory + 'static,
{
    /// Create new simple blocking server.
    pub fn new(transport: T, codec_factory: F) -> Self {
        Self {
            transport,
            codec_factory,
            services: HashMap::new(),
            running: false,
        }
    }

    fn process_request(&mut self, data: Vec<u8>) -> ErpcResult<Option<Vec<u8>>> {
        let mut codec = self.codec_factory.create_from_data(data);
        let message_info = codec.start_read_message()?;

        debug!(
            "Processing blocking request: type={}, service={}, method={}, sequence={}",
            message_info.message_type,
            message_info.service,
            message_info.request,
            message_info.sequence
        );

        match message_info.message_type {
            MessageType::Invocation | MessageType::Oneway => {}
            _ => return Err(RequestError::InvalidMessageType.into()),
        }

        let service = self
            .services
            .get(&message_info.service)
            .ok_or(RequestError::InvalidServiceId(message_info.service as u32))?;

        service.handle_invocation(message_info.request, message_info.sequence, &mut codec)?;

        if message_info.message_type == MessageType::Invocation {
            Ok(Some(codec.as_bytes().to_vec()))
        } else {
            Ok(None)
        }
    }

    /// Run while the provided predicate returns true.
    pub fn run_while<P>(&mut self, mut predicate: P) -> ErpcResult<()>
    where
        P: FnMut() -> bool,
    {
        self.running = true;
        info!("Blocking server started");

        while self.running && self.transport.is_connected() && predicate() {
            match self.poll() {
                Ok(_) => {}
                Err(crate::error::ErpcError::Transport(TransportError::Timeout)) => {}
                Err(e) => {
                    error!("Blocking server error: {}", e);
                    break;
                }
            }
        }

        self.running = false;
        info!("Blocking server stopped");
        Ok(())
    }
}

impl<T, F> BlockingServer for BlockingSimpleServer<T, F>
where
    T: BlockingTransport + 'static,
    F: CodecFactory + 'static,
{
    fn add_service(&mut self, service: Arc<dyn BlockingService>) -> ErpcResult<()> {
        let service_id = service.service_id();
        if self.services.contains_key(&service_id) {
            warn!("Blocking service {} already exists, replacing", service_id);
        }
        self.services.insert(service_id, service);
        info!("Added blocking service {}", service_id);
        Ok(())
    }

    fn remove_service(&mut self, service_id: u8) -> ErpcResult<()> {
        if self.services.remove(&service_id).is_some() {
            info!("Removed blocking service {}", service_id);
            Ok(())
        } else {
            Err(RequestError::InvalidServiceId(service_id as u32).into())
        }
    }

    fn poll(&mut self) -> ErpcResult<bool> {
        let data = match self.transport.receive() {
            Ok(data) => data,
            Err(crate::error::ErpcError::Transport(TransportError::Timeout)) => return Ok(false),
            Err(e) => return Err(e),
        };
        match self.process_request(data)? {
            Some(response) => {
                self.transport.send(&response)?;
                Ok(true)
            }
            None => Ok(true),
        }
    }

    fn run(&mut self) -> ErpcResult<()> {
        self.run_while(|| true)
    }

    fn stop(&mut self) -> ErpcResult<()> {
        self.running = false;
        self.transport.close()
    }

    fn is_running(&self) -> bool {
        self.running
    }
}

/// Blocking base service implementation with method routing.
pub struct BlockingBaseService {
    service_id: u8,
    methods: HashMap<u8, Box<dyn BlockingMethodHandler>>,
}

impl BlockingBaseService {
    /// Create new blocking base service.
    pub fn new(service_id: u8) -> Self {
        Self {
            service_id,
            methods: HashMap::new(),
        }
    }

    /// Add method handler.
    pub fn add_method<H>(&mut self, method_id: u8, handler: H)
    where
        H: BlockingMethodHandler + 'static,
    {
        self.methods.insert(method_id, Box::new(handler));
    }
}

impl BlockingService for BlockingBaseService {
    fn service_id(&self) -> u8 {
        self.service_id
    }

    fn handle_invocation(
        &self,
        method_id: u8,
        sequence: u32,
        codec: &mut dyn Codec,
    ) -> ErpcResult<()> {
        let handler = self
            .methods
            .get(&method_id)
            .ok_or_else(|| RequestError::InvalidMethodId(method_id as u32))?;
        handler.handle(sequence, codec)
    }

    fn supported_methods(&self) -> Vec<u8> {
        self.methods.keys().copied().collect()
    }
}

/// Blocking method handler trait.
pub trait BlockingMethodHandler: Send + Sync {
    /// Handle method call.
    fn handle(&self, sequence: u32, codec: &mut dyn Codec) -> ErpcResult<()>;
}

/// Function-based blocking method handler.
pub struct BlockingFunctionHandler<F>
where
    F: Fn(u32, &mut dyn Codec) -> ErpcResult<()> + Send + Sync,
{
    func: F,
}

impl<F> BlockingFunctionHandler<F>
where
    F: Fn(u32, &mut dyn Codec) -> ErpcResult<()> + Send + Sync,
{
    /// Create new blocking function handler.
    pub fn new(func: F) -> Self {
        Self { func }
    }
}

impl<F> BlockingMethodHandler for BlockingFunctionHandler<F>
where
    F: Fn(u32, &mut dyn Codec) -> ErpcResult<()> + Send + Sync,
{
    fn handle(&self, sequence: u32, codec: &mut dyn Codec) -> ErpcResult<()> {
        (self.func)(sequence, codec)
    }
}

/// Server builder for easy blocking configuration.
pub struct BlockingServerBuilder<T, F>
where
    T: BlockingTransport,
    F: CodecFactory,
{
    transport: Option<T>,
    codec_factory: Option<F>,
    services: Vec<Arc<dyn BlockingService>>,
}

impl<T, F> BlockingServerBuilder<T, F>
where
    T: BlockingTransport + 'static,
    F: CodecFactory + 'static,
{
    /// Create new blocking server builder.
    pub fn new() -> Self {
        Self {
            transport: None,
            codec_factory: None,
            services: Vec::new(),
        }
    }

    /// Set transport.
    pub fn transport(mut self, transport: T) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Set codec factory.
    pub fn codec_factory(mut self, codec_factory: F) -> Self {
        self.codec_factory = Some(codec_factory);
        self
    }

    /// Add service.
    pub fn service(mut self, service: Arc<dyn BlockingService>) -> Self {
        self.services.push(service);
        self
    }

    /// Build server.
    pub fn build(self) -> Result<BlockingSimpleServer<T, F>, &'static str> {
        let transport = self.transport.ok_or("Transport not set")?;
        let codec_factory = self.codec_factory.ok_or("Codec factory not set")?;
        let mut server = BlockingSimpleServer::new(transport, codec_factory);

        for service in self.services {
            server
                .add_service(service)
                .map_err(|_| "Failed to add service")?;
        }

        Ok(server)
    }
}

impl<T, F> Default for BlockingServerBuilder<T, F>
where
    T: BlockingTransport + 'static,
    F: CodecFactory + 'static,
{
    fn default() -> Self {
        Self::new()
    }
}
