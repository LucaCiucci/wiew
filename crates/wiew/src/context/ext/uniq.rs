use crate::context::WCx;

pub trait UseUniq {
    fn uniq<T: DedupUniq>(&mut self) -> &mut T;
    fn has_uniq<T: DedupUniq>(&self) -> Option<&T>;
    fn has_uniq_mut<T: DedupUniq>(&mut self) -> Option<&mut T>;
}

pub trait DedupUniq: Default + 'static {
    fn build(cx: &mut WCx) -> Self;
}

impl<T: Default + 'static> DedupUniq for T {
    fn build(_cx: &mut WCx) -> Self {
        Default::default()
    }
}

impl UseUniq for WCx {
    fn uniq<T: DedupUniq>(&mut self) -> &mut T {
        unimplemented!("Use WCx::pipeline for context-local caches")
    }

    fn has_uniq<T: DedupUniq>(&self) -> Option<&T> {
        None
    }

    fn has_uniq_mut<T: DedupUniq>(&mut self) -> Option<&mut T> {
        None
    }
}
