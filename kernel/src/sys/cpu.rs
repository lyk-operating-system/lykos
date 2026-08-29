#[repr(C)]
pub struct Cpu {
    #[cfg(target_arch = "x86_64")]
    pub self_ptr: *mut Cpu,
    pub cpu_id: usize,
}
