/*
 * @author t0bx
 * @since 23.10.2026
 */

pub type RequestId = u64;

#[derive(Debug, Default)]
pub struct RequestCounter {
    last: RequestId,
}

impl RequestCounter {
    pub fn allocate(&mut self) -> RequestId {
        self.last += 1;
        self.last
    }
}
