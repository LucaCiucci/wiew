use std::sync::Arc;

use wgpu::util::DeviceExt;

use crate::{
    context::WCx,
    resource::{H, Res},
};

pub trait BufElement: bytemuck::Pod + bytemuck::Zeroable + Send + Sync + 'static {}

impl<T: bytemuck::Pod + bytemuck::Zeroable + Send + Sync + 'static> BufElement for T {}

pub struct Buf<T: BufElement> {
    res: Res<BufDescriptor<T>>,
}

impl<T: BufElement> Clone for Buf<T> {
    fn clone(&self) -> Self {
        Self {
            res: self.res.clone(),
        }
    }
}

impl<T: BufElement> Buf<T> {
    pub fn new(usage: wgpu::BufferUsages, data: Vec<T>) -> Self {
        let descriptor = BufDescriptor {
            label: None,
            usage,
            data: Arc::new(BufSource::Vec(data)),
        };
        Self {
            res: Res::new(descriptor),
        }
    }

    pub fn with_label(self, label: impl Into<String>) -> Self {
        let descriptor = self.res.source().clone();
        let new_descriptor = BufDescriptor {
            label: Some(label.into()),
            usage: descriptor.usage,
            data: descriptor.data.clone(),
        };
        self.res.update(new_descriptor);
        self
    }

    pub fn with_loader(
        self,
        data: impl Fn() -> anyhow::Result<Vec<T>> + Send + Sync + 'static,
    ) -> Self {
        self.set_data_loader(data);
        self
    }

    pub fn set_data(&self, data: Vec<T>) {
        let snapshot = self.res.source();
        let descriptor = BufDescriptor {
            label: snapshot.label.clone(),
            usage: snapshot.usage,
            data: Arc::new(BufSource::Vec(data)),
        };
        self.res.update(descriptor);
    }

    pub fn set_data_arc_vec(&self, data: Arc<Vec<T>>) {
        let snapshot = self.res.source();
        let descriptor = BufDescriptor {
            label: snapshot.label.clone(),
            usage: snapshot.usage,
            data: Arc::new(BufSource::ArcVec(data)),
        };
        self.res.update(descriptor);
    }

    pub fn set_data_arc_slice(&self, data: Arc<[T]>) {
        let snapshot = self.res.source();
        let descriptor = BufDescriptor {
            label: snapshot.label.clone(),
            usage: snapshot.usage,
            data: Arc::new(BufSource::ArcSlice(data)),
        };
        self.res.update(descriptor);
    }

    pub fn set_data_loader(
        &self,
        data: impl Fn() -> anyhow::Result<Vec<T>> + Send + Sync + 'static,
    ) {
        let snapshot = self.res.source();
        let descriptor = BufDescriptor {
            label: snapshot.label.clone(),
            usage: snapshot.usage,
            data: Arc::new(BufSource::Loader(Box::new(data))),
        };
        self.res.update(descriptor);
    }

    pub fn len_hint(&self) -> Option<usize> {
        // TODO actually this could be exact if we store the length alongside the loader
        match &*self.res.source().data {
            BufSource::Vec(data) => Some(data.len()),
            BufSource::ArcVec(data) => Some(data.len()),
            BufSource::ArcSlice(data) => Some(data.len()),
            BufSource::Loader(_) => None,
        }
    }

    pub fn to_vec(&self) -> Option<Vec<T>> {
        match &*self.res.source().data {
            BufSource::Vec(data) => Some(data.clone()),
            BufSource::ArcVec(data) => Some((&**data).to_vec()),
            BufSource::ArcSlice(data) => Some((&**data).to_vec()),
            BufSource::Loader(_) => None,
        }
    }

    pub fn is_empty(&self) -> Option<bool> {
        self.len_hint().map(|len| len == 0)
    }

    pub fn get(&self, cx: &mut WCx) -> H<GpuBuffer> {
        let descriptor = self.res.source();

        cx.resources
            .clone()
            .get_or_instantiate_with(&self.res, |source, old_value: Option<GpuBuffer>| {
                let data_vec: Vec<T>;
                let data: &[T] = match &*descriptor.data {
                    BufSource::Vec(vec) => vec.as_slice(),
                    BufSource::ArcVec(vec) => vec.as_slice(),
                    BufSource::ArcSlice(slice) => slice.as_ref(),
                    BufSource::Loader(data_provider) => {
                        data_vec = cx.catch_err(data_provider()).unwrap_or_default();
                        data_vec.as_slice()
                    }
                };

                let byte_size = std::mem::size_of_val(data) as u64;

                if let Some(old) = old_value {
                    if old.usage == source.usage
                        && old.capacity >= byte_size
                        && old.usage.contains(wgpu::BufferUsages::COPY_DST)
                    {
                        cx.queue
                            .write_buffer(&old.buffer, 0, bytemuck::cast_slice(data));
                        return GpuBuffer {
                            buffer: old.buffer,
                            len: data.len() as u32,
                            capacity: old.capacity,
                            usage: old.usage,
                        };
                    }
                }

                let buffer = cx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: source.label.as_deref(),
                        contents: bytemuck::cast_slice(data),
                        usage: source.usage,
                    });

                GpuBuffer {
                    buffer,
                    len: data.len() as u32,
                    capacity: byte_size,
                    usage: source.usage,
                }
            })
            .clone()
    }
}

pub struct GpuBuffer {
    pub buffer: wgpu::Buffer,
    pub len: u32,
    pub capacity: u64,
    pub usage: wgpu::BufferUsages,
}

impl std::ops::Deref for GpuBuffer {
    type Target = wgpu::Buffer;

    fn deref(&self) -> &Self::Target {
        &self.buffer
    }
}

struct BufDescriptor<T> {
    label: Option<String>,
    usage: wgpu::BufferUsages,
    data: Arc<BufSource<T>>,
}

enum BufSource<T> {
    Vec(Vec<T>),
    ArcVec(Arc<Vec<T>>),
    ArcSlice(Arc<[T]>),
    Loader(Box<dyn Fn() -> anyhow::Result<Vec<T>> + Send + Sync>),
}
