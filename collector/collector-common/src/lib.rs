#![no_std]

// GpuActivity is how often, and how recently, a process has driven a GPU device.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuActivity {
    pub count: u64,
    pub last_ns: u64,
}

#[cfg(feature = "user")]
// SAFETY: repr(C), two u64 fields, no padding or invalid bit patterns.
unsafe impl aya::Pod for GpuActivity {}
