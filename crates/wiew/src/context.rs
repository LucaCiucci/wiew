use std::sync::Mutex;

use type_map::TypeMap;

use crate::resource::ResourceManager;

pub mod ext;

#[derive(Debug, thiserror::Error)]
pub enum CreateContextError {
    #[error("failed to request wgpu adapter: {0}")]
    RequestAdapter(wgpu::RequestAdapterError),
    #[error("failed to request wgpu device: {0}")]
    RequestDevice(wgpu::RequestDeviceError),
}

/// The main GPU context for wiew.
///
/// This is a wrapper around [`wgpu::Device`], [`wgpu::Queue`], and additional
/// support utilities for resource management.
pub struct WCx {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub(crate) storage: Storage,
    pub(crate) resources: ResourceManager,
    pub errors: Vec<anyhow::Error>,
}

impl WCx {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            device,
            queue,
            storage: Storage::new(),
            resources: ResourceManager::new(),
            errors: Vec::new(),
        }
    }

    pub fn push_error(&mut self, error: anyhow::Error) {
        self.errors.push(error);
    }

    pub fn catch_err<T, E>(&mut self, result: Result<T, E>) -> Option<T>
    where
        E: Into<anyhow::Error>,
    {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                self.push_error(error.into());
                None
            }
        }
    }

    pub fn drain_errors(&mut self) -> Vec<anyhow::Error> {
        std::mem::take(&mut self.errors)
    }

    pub async fn new_headless() -> Result<Self, CreateContextError> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(CreateContextError::RequestAdapter)?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("wiew headless device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                experimental_features: wgpu::ExperimentalFeatures::disabled(),
                memory_hints: wgpu::MemoryHints::Performance,
                trace: wgpu::Trace::Off,
            })
            .await
            .map_err(CreateContextError::RequestDevice)?;

        Ok(Self::new(device, queue))
    }
}

pub struct Storage {
    pub(crate) data: Mutex<TypeMap>,
}

impl Storage {
    pub fn new() -> Self {
        Self {
            data: Mutex::new(TypeMap::new()),
        }
    }
}
