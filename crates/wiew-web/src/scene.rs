use wiew::provided::{Grid, PcLod, QuadBackground, QuadBackgroundConfig, TrackballGizmo};

use crate::{
    ADDITIONAL_OBJECTS,
    loader::{PENDING_LOD, PENDING_LOD_PAYLOADS, payload_chunk_offset, queue_payload_request},
};

const MAX_LOD_PAYLOAD_APPLIES_PER_FRAME: usize = 1;

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
        if let Some(ref mut lod) = self.ply_lod {
            PENDING_LOD_PAYLOADS.with(|payloads| {
                let mut payloads = payloads.borrow_mut();
                for _ in 0..MAX_LOD_PAYLOAD_APPLIES_PER_FRAME {
                    let Some((chunk_index, payloads)) = payloads.pop_front() else {
                        break;
                    };
                    if let Err(err) =
                        lod.apply_cache_payload_chunk(payload_chunk_offset(chunk_index), &payloads)
                    {
                        log::error!("failed to apply PcLod payloads: {err}");
                    }
                }
            });
        }

        pass.draw(cx, &self.bg);
        pass.draw(cx, &self.grid);
        if let Some(ref mut lod) = self.ply_lod {
            pass.draw(cx, lod);
            for request in
                lod.take_requested_cache_payload_chunks(wiew::provided::PC_LOD_PAYLOAD_CHUNK_SIZE)
            {
                queue_payload_request(request.chunk_index);
            }
        }
        ADDITIONAL_OBJECTS.with(|objs| {
            for obj in objs.borrow().iter() {
                obj.draw(cx, pass);
            }
        });
        pass.draw(cx, &self.gizmo);
    }
}
