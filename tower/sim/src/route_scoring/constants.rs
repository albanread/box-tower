// ts: route-scoring/constants.ts

// Route-scoring shared constants. Not a binary-named function; hosts the
// cost-infinite sentinel and the stairs-extra-cost offset referenced by
// the per-scorer files in this directory.

pub const ROUTE_COST_INFINITE: i64 = 0x7fff;
pub const STAIRS_ROUTE_EXTRA_COST: i64 = 0x280; // 640
pub const DIRECT_ROUTE_BASE_COST: i64 = 0x280; // 640 (11b8:168e direct branch)
pub const DIRECT_ROUTE_FULL_QUEUE_COST: i64 = 0x3e8; // 1000 (replaces base when qByte == 0x28, mode != 0)
pub const TRANSFER_ROUTE_BASE_COST: i64 = 0xbb8; // 3000 (11b8:168e transfer branch)
pub const TRANSFER_ROUTE_FULL_QUEUE_COST: i64 = 0x1770; // 6000 (replaces base when qByte == 0x28, mode != 0)
pub const QUEUE_FULL_COUNT: i64 = 0x28; // 40 (ring buffer capacity)
