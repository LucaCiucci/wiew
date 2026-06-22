// NOTE: 0 is reserved for the default vertex buffer
static ID_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(10);

pub fn new_id_value() -> u64 {
    ID_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}
