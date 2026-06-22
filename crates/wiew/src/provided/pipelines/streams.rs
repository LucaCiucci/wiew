use crate::{
    GpuBuffer, WCx,
    mesh::{Color, Mesh, MeshStreamId, Normal},
    resource::H,
};

pub(crate) struct PositionColorBuffers {
    pub positions: H<GpuBuffer>,
    pub colors: H<GpuBuffer>,
    pub vertex_count: u32,
}

pub(crate) struct PositionNormalBuffers {
    pub positions: H<GpuBuffer>,
    pub normals: H<GpuBuffer>,
    pub vertex_count: u32,
}

pub(crate) struct PositionNormalColorBuffers {
    pub positions: H<GpuBuffer>,
    pub normals: H<GpuBuffer>,
    pub colors: H<GpuBuffer>,
    pub vertex_count: u32,
}

pub(crate) fn bind_position_color_mesh(
    cx: &mut WCx,
    mesh: &Mesh,
    pipeline_name: &str,
) -> Option<PositionColorBuffers> {
    let positions = mesh.bind_positions(cx);
    let colors = mesh.bind_stream::<Color>(cx, MeshStreamId::COLOR)?;

    if positions.len == 0 {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} requires at least one vertex"
        ));
        return None;
    }

    if positions.len != colors.len {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} buffer length mismatch: positions={}, colors={}",
            positions.len,
            colors.len
        ));
        return None;
    }

    let vertex_count = positions.len;

    Some(PositionColorBuffers {
        positions,
        colors,
        vertex_count,
    })
}

pub(crate) fn bind_position_normal_mesh(
    cx: &mut WCx,
    mesh: &Mesh,
    pipeline_name: &str,
) -> Option<PositionNormalBuffers> {
    let positions = mesh.bind_positions(cx);
    let normals = mesh.bind_stream::<Normal>(cx, MeshStreamId::NORMAL)?;

    if positions.len == 0 {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} requires at least one vertex"
        ));
        return None;
    }

    if positions.len != normals.len {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} buffer length mismatch: positions={}, normals={}",
            positions.len,
            normals.len
        ));
        return None;
    }

    let vertex_count = positions.len;

    Some(PositionNormalBuffers {
        positions,
        normals,
        vertex_count,
    })
}

pub(crate) fn bind_position_normal_color_mesh(
    cx: &mut WCx,
    mesh: &Mesh,
    pipeline_name: &str,
) -> Option<PositionNormalColorBuffers> {
    let positions = mesh.bind_positions(cx);
    let normals = mesh.bind_stream::<Normal>(cx, MeshStreamId::NORMAL)?;
    let colors = mesh.bind_stream::<Color>(cx, MeshStreamId::COLOR)?;

    if positions.len == 0 {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} requires at least one vertex"
        ));
        return None;
    }

    if positions.len != normals.len || positions.len != colors.len {
        cx.push_error(anyhow::anyhow!(
            "{pipeline_name} buffer length mismatch: positions={}, normals={}, colors={}",
            positions.len,
            normals.len,
            colors.len
        ));
        return None;
    }

    let vertex_count = positions.len;

    Some(PositionNormalColorBuffers {
        positions,
        normals,
        colors,
        vertex_count,
    })
}
