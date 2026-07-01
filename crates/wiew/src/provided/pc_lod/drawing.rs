use super::*;

impl Drawable for PcLod {
    fn draw(&self, cx: &mut WCx, pass: &mut Pass) {
        let mut selection = self.select(pass);
        selection.finish_stats(self);
        *self.last_stats.lock().unwrap() = selection.stats;

        if !selection.proxies.is_empty() {
            let mut positions = Vec::with_capacity(selection.proxies.len());
            let mut normals = Vec::with_capacity(selection.proxies.len());
            let mut colors = Vec::with_capacity(selection.proxies.len());
            for proxy in selection.proxies {
                positions.push(proxy.position);
                normals.push(proxy.normal);
                colors.push(proxy.color);
            }
            let mut proxy_mesh = self.proxy_mesh.lock().unwrap();
            proxy_mesh.set_positions(positions);
            proxy_mesh.set_normals(normals);
            proxy_mesh.set_colors(colors);
            self.pipeline
                .draw_mesh_with_material(cx, pass, &proxy_mesh, &self.material);
        }

        for selected in selection.node_lods {
            if let Some(mesh) = self
                .node_lods
                .get(selected.node_lods)
                .and_then(|lods| lods.get(selected.lod))
            {
                self.pipeline
                    .draw_mesh_with_material(cx, pass, mesh, &self.material);
            }
        }

        for leaf_mesh in selection.leaf_meshes {
            let mesh = match leaf_mesh.lod {
                Some(lod) => self
                    .leaf_lods
                    .get(leaf_mesh.leaf)
                    .and_then(|lods| lods.get(lod)),
                None => self.leaf_meshes.get(leaf_mesh.leaf),
            };
            if let Some(mesh) = mesh {
                self.pipeline
                    .draw_mesh_with_material(cx, pass, mesh, &self.material);
            }
        }

        if self.draw_bounds {
            let (positions, colors) = bounds_lines(&selection.bounds);
            let mut bounds_mesh = self.bounds_mesh.lock().unwrap();
            bounds_mesh.set_positions(positions);
            bounds_mesh.set_colors(colors);
            self.bounds_pipeline.draw_mesh(cx, pass, &bounds_mesh);
        }
    }
}


fn bounds_lines(bounds: &[(Aabb, PcLodBoundsKind)]) -> (Vec<Position>, Vec<Color>) {
    let mut positions = Vec::with_capacity(bounds.len() * 24);
    let mut colors = Vec::with_capacity(bounds.len() * 24);

    for (bounds, kind) in bounds {
        let color = match kind {
            PcLodBoundsKind::Proxy => [0.15, 0.95, 1.0, 0.95],
            PcLodBoundsKind::NodeLod => [1.0, 0.45, 0.0, 0.85],
            PcLodBoundsKind::Leaf => [1.0, 0.85, 0.10, 0.75],
        };
        let corners = bounds.corners();
        for (a, b) in AABB_EDGES {
            positions.push(point_to_position(corners[a]));
            positions.push(point_to_position(corners[b]));
            colors.push(color);
            colors.push(color);
        }
    }

    (positions, colors)
}

const AABB_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (1, 3),
    (3, 2),
    (2, 0),
    (4, 5),
    (5, 7),
    (7, 6),
    (6, 4),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];
