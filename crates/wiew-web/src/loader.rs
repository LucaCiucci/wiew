use std::{cell::RefCell, collections::VecDeque, io::Cursor};

use js_sys::Array;
use wasm_bindgen::prelude::*;
use wiew::{
    mesh::{Color, Normal, Position},
    provided::{PC_LOD_PAYLOAD_CHUNK_SIZE, PcLod, PcLodConfig},
};

// ---------------------------------------------------------------------------
// Pending LOD queue – load_ply_xz pushes, Scene::render picks up
// ---------------------------------------------------------------------------

thread_local! {
    pub(crate) static PENDING_LOD: RefCell<Option<PcLod>> = const { RefCell::new(None) };
    pub(crate) static PENDING_LOD_PAYLOADS: RefCell<VecDeque<(usize, Vec<u8>)>> = const { RefCell::new(VecDeque::new()) };
    pub(crate) static PENDING_LOD_PAYLOAD_REQUESTS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
}

// ---------------------------------------------------------------------------
// Wasm entry point – callable from JavaScript
// ---------------------------------------------------------------------------

/// Fetch an XZ-compressed binary PLY from JavaScript and build a `PcLod`.
///
/// The PLY must contain at least `x y z nx ny nz` per vertex (6 × f32 = 24
/// bytes per point after the ASCII header).  Colors default to white.
#[wasm_bindgen]
pub fn load_ply_xz(compressed: &[u8]) -> Result<(), JsValue> {
    // 1. Decompress
    let mut ply_bytes = Vec::new();
    lzma_rs::xz_decompress(&mut Cursor::new(compressed), &mut ply_bytes)
        .map_err(|e| JsValue::from_str(&format!("xz decompress failed: {e}")))?;

    // 2. Parse PLY binary data
    let (positions, normals, colors) = parse_ply(&ply_bytes)?;

    // 3. Build LOD tree
    let lod = PcLod::from_streams(
        positions,
        normals,
        colors,
        PcLodConfig {
            leaf_point_count: 65_536,
            proxy_diameter_px: 2.5,
            points_per_pixel: 1.05,
            node_lod_point_count: 8_192,
            max_depth: 14,
        },
    )
    .map_err(|e| JsValue::from_str(&format!("PcLod build failed: {e}")))?;

    // 4. Push into the pending queue
    PENDING_LOD.with(|p| *p.borrow_mut() = Some(lod));

    Ok(())
}

#[wasm_bindgen]
pub fn load_wlod_metadata(metadata: &[u8]) -> Result<(), JsValue> {
    let lod = PcLod::from_cache_metadata_bytes(metadata)
        .map_err(|e| JsValue::from_str(&format!("PcLod metadata load failed: {e}")))?;
    PENDING_LOD.with(|p| *p.borrow_mut() = Some(lod));
    Ok(())
}

#[wasm_bindgen]
pub fn load_wlod_payloads(payloads: &[u8]) -> Result<(), JsValue> {
    PENDING_LOD_PAYLOADS.with(|p| p.borrow_mut().push_back((0, payloads.to_vec())));
    Ok(())
}

#[wasm_bindgen]
pub fn load_wlod_payload_chunk(chunk_index: usize, payloads: &[u8]) -> Result<(), JsValue> {
    PENDING_LOD_PAYLOADS.with(|p| p.borrow_mut().push_back((chunk_index, payloads.to_vec())));
    Ok(())
}

#[wasm_bindgen]
pub fn take_wlod_payload_requests() -> Array {
    let requests = Array::new();
    PENDING_LOD_PAYLOAD_REQUESTS.with(|pending| {
        let mut pending = pending.borrow_mut();
        for chunk_index in pending.iter() {
            requests.push(&JsValue::from_f64(*chunk_index as f64));
        }
        pending.clear();
    });
    requests
}

pub(crate) fn payload_chunk_offset(chunk_index: usize) -> usize {
    chunk_index * PC_LOD_PAYLOAD_CHUNK_SIZE
}

pub(crate) fn queue_payload_request(chunk_index: usize) {
    PENDING_LOD_PAYLOAD_REQUESTS.with(|pending| {
        let mut pending = pending.borrow_mut();
        if !pending.contains(&chunk_index) {
            pending.push(chunk_index);
        }
    });
}

// ---------------------------------------------------------------------------
// PLY parser (binary LE, xyz + nx ny nz, 6 × f32 = 24 bytes per vertex)
// ---------------------------------------------------------------------------

fn parse_ply(ply_bytes: &[u8]) -> Result<(Vec<Position>, Vec<Normal>, Vec<Color>), JsValue> {
    let header_end = ply_bytes
        .windows(b"end_header\n".len())
        .position(|w| w == b"end_header\n")
        .ok_or_else(|| JsValue::from_str("PLY header: missing end_header"))?
        + b"end_header\n".len();

    const VERTEX_STRIDE: usize = 24;
    let data = &ply_bytes[header_end..];
    let count = data.len() / VERTEX_STRIDE;

    if count == 0 {
        return Err(JsValue::from_str("PLY contains no vertices"));
    }

    let mut positions = Vec::<Position>::with_capacity(count);
    let mut normals = Vec::<Normal>::with_capacity(count);
    let mut colors = Vec::<Color>::with_capacity(count);

    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for i in 0..count {
        let off = i * VERTEX_STRIDE;
        let chunk = &data[off..off + VERTEX_STRIDE];

        let x = f32::from_le_bytes(chunk[0..4].try_into().unwrap());
        let y = f32::from_le_bytes(chunk[4..8].try_into().unwrap());
        let z = f32::from_le_bytes(chunk[8..12].try_into().unwrap());
        let nx = f32::from_le_bytes(chunk[12..16].try_into().unwrap());
        let ny = f32::from_le_bytes(chunk[16..20].try_into().unwrap());
        let nz = f32::from_le_bytes(chunk[20..24].try_into().unwrap());

        let p = [x, y, z];
        for a in 0..3 {
            min[a] = min[a].min(p[a]);
            max[a] = max[a].max(p[a]);
        }
        positions.push(p);

        let len = (nx * nx + ny * ny + nz * nz).sqrt();
        normals.push(if len == 0.0 {
            [0.0, 1.0, 0.0]
        } else {
            [nx / len, ny / len, nz / len]
        });

        colors.push([1.0, 1.0, 1.0, 1.0]);
    }

    // Normalise to unit cube (~[-1.5, 1.5])
    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let extent = [
        (max[0] - min[0]).max(0.0001),
        (max[1] - min[1]).max(0.0001),
        (max[2] - min[2]).max(0.0001),
    ];
    let scale = 3.0 / extent[0].max(extent[1]).max(extent[2]);

    for p in &mut positions {
        p[0] = (p[0] - center[0]) * scale;
        p[1] = (p[1] - center[1]) * scale;
        p[2] = (p[2] - center[2]) * scale;
    }

    Ok((positions, normals, colors))
}
