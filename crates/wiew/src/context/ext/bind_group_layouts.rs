use std::{collections::HashMap, hash::Hash, sync::Arc};

use crate::context::WCx;

pub trait BindGroupLayout: Clone + Eq + Hash + Send + Sync + 'static {
    fn build(&self, cx: &mut WCx) -> wgpu::BindGroupLayout;
}

pub trait UseBindGroupLayouts {
    fn use_bind_group_layout<L: BindGroupLayout>(
        &mut self,
        layout: &L,
    ) -> Arc<wgpu::BindGroupLayout>;
}

impl UseBindGroupLayouts for WCx {
    fn use_bind_group_layout<L: BindGroupLayout>(
        &mut self,
        layout: &L,
    ) -> Arc<wgpu::BindGroupLayout> {
        {
            let mut storage = self.storage.data.lock().unwrap();
            let cache = storage
                .entry::<BindGroupLayoutCache<L>>()
                .or_insert_with(BindGroupLayoutCache::default);

            if let Some(layout) = cache.layouts.get(layout) {
                return Arc::clone(layout);
            }
        }

        let wgpu_layout = Arc::new(layout.build(self));

        let mut storage = self.storage.data.lock().unwrap();
        let cache = storage
            .entry::<BindGroupLayoutCache<L>>()
            .or_insert_with(BindGroupLayoutCache::default);

        Arc::clone(
            cache
                .layouts
                .entry(layout.clone())
                .or_insert_with(|| Arc::clone(&wgpu_layout)),
        )
    }
}

struct BindGroupLayoutCache<L> {
    layouts: HashMap<L, Arc<wgpu::BindGroupLayout>>,
}

impl<L> Default for BindGroupLayoutCache<L> {
    fn default() -> Self {
        Self {
            layouts: HashMap::new(),
        }
    }
}
