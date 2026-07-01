use crate::{Pass, WCx};

/// An object that can be drawn into a render pass.
pub trait Drawable {
    /// Draw the component into an active render pass.
    ///
    /// All GPU resources are obtained from `cx.resources` (via the
    /// managed wrappers), so the same component works across different
    /// [`WCx`] contexts.
    fn draw(&self, cx: &mut WCx, pass: &mut Pass);
}

impl Drawable for () {
    fn draw(&self, _cx: &mut WCx, _pass: &mut Pass) {}
}
