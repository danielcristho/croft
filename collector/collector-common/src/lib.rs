#![no_std]

#[repr(C)]
#[derive(Clone, Copy)]
pub struct GpuActivity {
    pub count: u64,
    pub last_ns: u64,
}

#[cfg(feature = "user")]
unsafe impl aya::Pod for GpuActivity {}
