//! Resource management utilities for wiew.
//!
//! In general, a resource is any object that is related to a specific wgpu context
//! and it may not be possible to hold it outside of that context, or it may be
//! expensive to create and destroy it frequently. Examples of resources include
//! wgpu buffers, textures, and pipelines.
//!
//! This module provides a [`ResourceManager`] that can be used to manage
//! resources from within the wiew system. This way the lifetime of such
//! resources can be explicitly managed and they can be reused.
//!
//! A resource should be fully described by a source value. It may require
//! additional context (e.g. a [`wgpu::Queue`] or a [`wgpu::Device`]) to be
//! instantiated/modified, but in principle it should not depend on any other
//! state. See [`ResourceManager::get_or_instantiate_with`] for more details on
//! the instantiation process.

use std::{
    any::Any,
    collections::HashMap,
    fmt::Display,
    sync::{Arc, Mutex, Weak},
};

use crate::id::new_id_value;

#[derive(Clone)]
pub(crate) struct ResourceManager {
    inner: Arc<Mutex<ResourceManagerInner>>,
}

impl ResourceManager {
    pub(crate) fn new() -> Self {
        ResourceManager {
            inner: Arc::new(Mutex::new(ResourceManagerInner {
                resources: HashMap::new(),
            })),
        }
    }

    /// Get a resource value from the manager, or instantiate it if it does not
    /// exist yet.
    ///
    /// The `instantiate` function is called with the resource's source and an
    /// optional previous value. It should return a new value for the resource.
    /// The previous value is provided only when the manager has the sole
    /// reference to the old cached value.
    ///
    /// No manager lock is held while `instantiate` runs. This lets
    /// instantiation resolve other resources through the same manager without
    /// deadlocking the manager itself.
    pub(crate) fn get_or_instantiate_with<'scope, S, V>(
        &'scope self,
        res: &Res<S>,
        instantiate: impl FnOnce(&S, Option<V>) -> V,
    ) -> H<V>
    where
        S: Send + Sync + 'static,
        V: Send + Sync + 'static,
    {
        let ResSnapshot {
            id,
            version,
            source,
            lifetime,
        } = res.snapshot();

        let old_value = {
            let mut inner = self.inner.lock().unwrap();
            // A resource manager lives for the wgpu context, while scene
            // resources are usually short lived. Evict GPU values whose
            // owning `Res` no longer exists before growing the cache.
            inner
                .resources
                .retain(|_, data| data.lifetime.upgrade().is_some());
            let data = inner.resources.entry(id).or_default();
            data.lifetime = lifetime;

            if data.curr_version == version {
                if let Some(existing) = data.typed_value::<V>() {
                    return H(existing);
                }
            }

            data.take_value::<V>()
        };

        let new_value = Arc::new(instantiate(&source, old_value));

        let mut inner = self.inner.lock().unwrap();
        let data = inner.resources.entry(id).or_default();

        if data.curr_version == version {
            if let Some(existing) = data.typed_value::<V>() {
                return H(existing);
            }
        }

        data.curr_value = Some(new_value.clone());
        data.curr_version = version;

        H(new_value)
    }
}

pub struct H<T: Send + Sync + 'static>(Arc<T>);

impl<T: Send + Sync + 'static> Clone for H<T> {
    fn clone(&self) -> Self {
        H(Arc::clone(&self.0))
    }
}

impl<T: Send + Sync + 'static> std::ops::Deref for H<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Default for ResourceManager {
    fn default() -> Self {
        Self::new()
    }
}

struct ResourceManagerInner {
    resources: HashMap<ResId, ResData>,
}

#[derive(Default)]
struct ResData {
    curr_version: u64,
    curr_value: Option<Arc<dyn Any + Send + Sync>>,
    lifetime: Weak<()>,
}

impl ResData {
    fn typed_value<V>(&self) -> Option<Arc<V>>
    where
        V: Send + Sync + 'static,
    {
        self.curr_value
            .as_ref()
            .and_then(|value| value.clone().downcast::<V>().ok())
    }

    fn take_value<V>(&mut self) -> Option<V>
    where
        V: Send + Sync + 'static,
    {
        if self
            .curr_value
            .as_ref()
            .is_some_and(|value| Arc::strong_count(value) != 1)
        {
            return None;
        }

        self.curr_value
            .take()
            .and_then(|value| value.downcast::<V>().ok())
            .and_then(|value| Arc::try_unwrap(value).ok())
    }
}

pub struct Res<S: Send + Sync + 'static> {
    inner: Arc<Mutex<ResInner<S>>>,
    lifetime: Arc<()>,
}

impl<S: Send + Sync + 'static> Res<S> {
    pub fn new(source: S) -> Self {
        Res {
            inner: Arc::new(Mutex::new(ResInner::new(Arc::new(source)))),
            lifetime: Arc::new(()),
        }
    }

    pub fn source(&self) -> Arc<S> {
        Arc::clone(&self.inner.lock().unwrap().source)
    }

    pub fn update(&self, new_source: S) {
        let mut inner = self.inner.lock().unwrap();
        inner.update(Arc::new(new_source));
    }

    pub fn update_if_changed(&self, new_source: S)
    where
        S: PartialEq,
    {
        let mut inner = self.inner.lock().unwrap();
        if *inner.source != new_source {
            inner.update(Arc::new(new_source));
        }
    }

    fn snapshot(&self) -> ResSnapshot<S> {
        let inner = self.inner.lock().unwrap();
        ResSnapshot {
            id: inner.id,
            version: inner.version,
            source: Arc::clone(&inner.source),
            lifetime: Arc::downgrade(&self.lifetime),
        }
    }
}

impl<S: Send + Sync + 'static> Clone for Res<S> {
    fn clone(&self) -> Self {
        Res {
            inner: Arc::clone(&self.inner),
            lifetime: Arc::clone(&self.lifetime),
        }
    }
}

struct ResSnapshot<S> {
    id: ResId,
    version: u64,
    source: Arc<S>,
    lifetime: Weak<()>,
}

struct ResInner<S> {
    id: ResId,
    version: u64,
    source: Arc<S>,
}

impl<S> ResInner<S> {
    fn new(source: Arc<S>) -> Self {
        ResInner {
            id: ResId::new(),
            version: 0,
            source,
        }
    }

    fn update(&mut self, new_source: Arc<S>) {
        self.source = new_source;
        self.version = self
            .version
            .checked_add(1)
            .expect("resource version overflow");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResId(u64);

impl ResId {
    pub fn new() -> Self {
        ResId(new_id_value())
    }
}

impl Display for ResId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ResId({})", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    type TextResource = Res<String>;
    type NumberResource = Res<usize>;

    #[test]
    fn returns_cached_value_for_unchanged_resource() {
        let manager = ResourceManager::new();
        let res = TextResource::new("source".to_owned());
        let calls = AtomicUsize::new(0);

        let first = manager.get_or_instantiate_with(&res, |source, old| {
            assert_eq!(source, "source");
            assert!(old.is_none());
            calls.fetch_add(1, Ordering::SeqCst);
            "value".to_owned()
        });
        let second = manager.get_or_instantiate_with(&res, |_, _| {
            panic!("cached resource should not be instantiated again")
        });

        assert!(Arc::ptr_eq(&first.0, &second.0));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn update_invalidates_cached_value_and_can_reuse_old_value() {
        let manager = ResourceManager::new();
        let res = TextResource::new("first".to_owned());

        let first = manager.get_or_instantiate_with(&res, |source, old| {
            assert_eq!(source, "first");
            assert!(old.is_none());
            "old value".to_owned()
        });
        drop(first);

        res.update("second".to_owned());

        let second = manager.get_or_instantiate_with(&res, |source, old| {
            assert_eq!(source, "second");
            assert_eq!(old.as_deref(), Some("old value"));
            "new value".to_owned()
        });

        assert_eq!(&*second, "new value");
    }

    #[test]
    fn update_does_not_move_old_value_while_external_references_exist() {
        let manager = ResourceManager::new();
        let res = TextResource::new("first".to_owned());

        let first = manager.get_or_instantiate_with(&res, |_, _| "old value".to_owned());
        res.update("second".to_owned());

        let second = manager.get_or_instantiate_with(&res, |_, old| {
            assert!(old.is_none());
            "new value".to_owned()
        });

        assert_eq!(&*first, "old value");
        assert_eq!(&*second, "new value");
    }

    #[test]
    fn instantiate_can_resolve_another_resource_through_same_manager() {
        let manager = ResourceManager::new();
        let outer = TextResource::new("outer".to_owned());
        let inner = NumberResource::new(40);

        let value = manager.get_or_instantiate_with(&outer, |source, old| {
            assert!(old.is_none());
            let number = manager.get_or_instantiate_with(&inner, |source, old| {
                assert!(old.is_none());
                source + 2
            });

            format!("{source}-{}", *number)
        });

        assert_eq!(&*value, "outer-42");
    }

    #[test]
    fn evicts_cached_values_after_their_resource_is_dropped() {
        let manager = ResourceManager::new();
        {
            let stale = TextResource::new("stale".to_owned());
            let value = manager.get_or_instantiate_with(&stale, |_, _| "value".to_owned());
            drop(value);
        }

        let live = TextResource::new("live".to_owned());
        let value = manager.get_or_instantiate_with(&live, |_, _| "value".to_owned());
        drop(value);

        assert_eq!(manager.inner.lock().unwrap().resources.len(), 1);
    }
}
