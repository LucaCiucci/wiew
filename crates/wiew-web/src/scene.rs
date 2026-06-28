use wiew::{
    provided::{Grid, PcLod, QuadBackground, QuadBackgroundConfig, TrackballGizmo},
};

use crate::{ADDITIONAL_OBJECTS, loader::PENDING_LOD};

// ---------------------------------------------------------------------------
// Scene
// ---------------------------------------------------------------------------

pub(crate) struct Scene {
    bg: QuadBackground,
    grid: Grid,
    gizmo: TrackballGizmo,
    ply_lod: Option<PcLod>,
}

impl Scene {
    pub(crate) fn new() -> Self {
        Self {
            bg: QuadBackground::new(QuadBackgroundConfig::default()),
            grid: Grid::new(10),
            gizmo: TrackballGizmo::new(),
            ply_lod: None,
        }
    }

    pub(crate) fn render(&mut self, cx: &mut wiew::WCx, pass: &mut wiew::Pass) {
        // Pick up any LOD that was pushed from JS
        if let Some(lod) = PENDING_LOD.with(|p| p.borrow_mut().take()) {
            self.ply_lod = Some(lod);
        }

        pass.draw(cx, &self.bg);
        pass.draw(cx, &self.grid);
        if let Some(ref mut lod) = self.ply_lod {
            pass.draw(cx, lod);
        }
        ADDITIONAL_OBJECTS.with(|objs| {
            for obj in objs.borrow().iter() {
                obj.draw(cx, pass);
            }
        });
        pass.draw(cx, &self.gizmo);
    }
}
