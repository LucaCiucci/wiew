use std::{collections::HashMap, hash::Hash, sync::Arc};

use crate::{context::WCx, render_target::TargetKey};

pub trait Pipeline: Clone + Eq + Hash + Send + Sync + 'static {
    fn build(&self, cx: &mut WCx, target: &TargetKey) -> wgpu::RenderPipeline;
}

pub trait UsePipelines {
    fn use_pipeline<P: Pipeline>(
        &mut self,
        target: &TargetKey,
        pipeline: &P,
    ) -> Arc<wgpu::RenderPipeline>;
}

impl UsePipelines for WCx {
    fn use_pipeline<P: Pipeline>(
        &mut self,
        target: &TargetKey,
        pipeline: &P,
    ) -> Arc<wgpu::RenderPipeline> {
        {
            let mut storage = self.storage.data.lock().unwrap();
            let cache = storage
                .entry::<PipelinesByTarget<P>>()
                .or_insert_with(PipelinesByTarget::default);

            if let Some(pipeline) = cache
                .pipelines
                .get(target)
                .and_then(|cache| cache.pipelines.get(pipeline))
            {
                return Arc::clone(pipeline);
            }
        }

        let wgpu_pipeline = Arc::new(pipeline.build(self, target));

        let mut storage = self.storage.data.lock().unwrap();
        let cache = storage
            .entry::<PipelinesByTarget<P>>()
            .or_insert_with(PipelinesByTarget::default);
        let target_cache = cache
            .pipelines
            .entry(target.clone())
            .or_insert_with(PipelineCache::default);
        target_cache
            .pipelines
            .insert(pipeline.clone(), Arc::clone(&wgpu_pipeline));
        wgpu_pipeline
    }
}

struct PipelinesByTarget<K> {
    pipelines: HashMap<TargetKey, PipelineCache<K>>,
}

impl<K> Default for PipelinesByTarget<K> {
    fn default() -> Self {
        Self {
            pipelines: HashMap::new(),
        }
    }
}

struct PipelineCache<K> {
    pipelines: HashMap<K, Arc<wgpu::RenderPipeline>>,
}

impl<K> Default for PipelineCache<K> {
    fn default() -> Self {
        Self {
            pipelines: HashMap::new(),
        }
    }
}
