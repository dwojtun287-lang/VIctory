//! Buffer uses for one GPU command. Each completed command returns its
//! buffers to COMMON, so lists can be recorded and submitted in any order,
//! including several lists in a single ExecuteCommandLists call.

use std::collections::{HashMap, HashSet};
use windows::Win32::Graphics::Direct3D12::*;
use windows::core::Interface;

#[derive(Default)]
pub(crate) struct BufferStates {
    pending: HashMap<usize, (ID3D12Resource, u32)>,
    active: Vec<(ID3D12Resource, u32)>,
    held: HashSet<usize>,
}

fn valid(state: u32) -> bool {
    // These buffers are used for reads or UAV writes. A write state cannot
    // be combined with read states, even when views cover different ranges.
    state & 8 == 0 || state == 8
}

impl BufferStates {
    pub(crate) fn require(&mut self, buffer: &ID3D12Resource, state: u32) -> bool {
        let key = buffer.as_raw() as usize;
        let entry = self
            .pending
            .entry(key)
            .or_insert_with(|| (buffer.clone(), 0));
        entry.1 |= state;
        self.held.insert(key)
    }

    pub(crate) fn apply(&mut self, list: &ID3D12GraphicsCommandList) -> bool {
        if self.pending.values().any(|(_, state)| !valid(*state)) {
            static DROPPED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let n = DROPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            if n.is_power_of_two() {
                tracing::warn!(
                    dropped = n,
                    "a draw or dispatch dropped: one host buffer needs UAV and read access simultaneously"
                );
            }
            self.pending.clear();
            return false;
        }
        assert!(self.active.is_empty());
        self.active
            .extend(self.pending.drain().map(|(_, entry)| entry));
        let barriers: Vec<_> = self
            .active
            .iter()
            .map(|(buffer, state)| {
                crate::host::transition(
                    buffer,
                    D3D12_RESOURCE_STATE_COMMON,
                    D3D12_RESOURCE_STATES(*state as i32),
                )
            })
            .collect();
        if !barriers.is_empty() {
            // SAFETY: an open list; all buffers are retained by the caller.
            unsafe { list.ResourceBarrier(&barriers) };
        }
        true
    }

    pub(crate) fn finish(&mut self, list: &ID3D12GraphicsCommandList) {
        let barriers: Vec<_> = self
            .active
            .drain(..)
            .map(|(buffer, state)| {
                crate::host::transition(
                    &buffer,
                    D3D12_RESOURCE_STATES(state as i32),
                    D3D12_RESOURCE_STATE_COMMON,
                )
            })
            .collect();
        if !barriers.is_empty() {
            // SAFETY: an open list; buffers are kept by its deferred work.
            unsafe { list.ResourceBarrier(&barriers) };
        }
        self.pending.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_states_combine_but_uav_is_exclusive() {
        assert!(valid(1 | 2 | 0x40 | 0x80 | 0x200));
        assert!(valid(8));
        for read in [1, 2, 0x40, 0x80, 0x200] {
            assert!(!valid(8 | read));
        }
    }

    #[test]
    fn write_then_vertex_and_index_read_across_lists() {
        let host = crate::host::host();
        if std::env::var_os("VICTORY_D3D12_WARP").is_some() {
            assert!(
                host.is_some(),
                "the requested WARP test device must be available"
            );
        }
        let Some(host) = host else { return };
        let (buffer, data) = host.shared_buffer(65536).unwrap();
        // SAFETY: the live CPU mapping holds one DRAW_INSTANCED record.
        unsafe { data.cast::<[u32; 4]>().write([1 << 25, 1, 0, 0]) };
        let first = host.new_list().unwrap();
        // Exercise the actual argument clamp, which used to leave a shared
        // vertex/argument buffer in UAV state after indirect execution.
        // SAFETY: a live buffer.
        let gpu = unsafe { buffer.GetGPUVirtualAddress() };
        assert!(crate::indirect::bound(&first, &buffer, gpu, (0, 16, 1)));
        first.require_buffer(&buffer, D3D12_RESOURCE_STATE_VERTEX_AND_CONSTANT_BUFFER);
        first.require_buffer(&buffer, D3D12_RESOURCE_STATE_INDEX_BUFFER);
        first.require_buffer(&buffer, D3D12_RESOURCE_STATE_INDIRECT_ARGUMENT);
        assert!(first.apply_buffers());
        first.close();
        let second = host.new_list().unwrap();
        second.require_buffer(&buffer, D3D12_RESOURCE_STATE_UNORDERED_ACCESS);
        assert!(second.apply_buffers());
        second.close();
        host.execute(&[&first, &second]);
        host.execute(&[&second, &first]);
        // SAFETY: the GPU work completed and the buffer remains mapped.
        assert_eq!(unsafe { data.cast::<u32>().read() }, 1 << 24);
        // SAFETY: the host owns a live device; execution waited for completion.
        unsafe { host.device.GetDeviceRemovedReason() }.unwrap();
        let queue = host.device.cast::<ID3D12InfoQueue>();
        if std::env::var_os("VICTORY_D3D12_DEBUG").is_some() {
            assert!(
                queue.is_ok(),
                "the requested D3D12 debug layer must be available"
            );
        }
        if let Ok(queue) = queue {
            // SAFETY: live queue; message storage is aligned and sized by GetMessage.
            unsafe {
                for i in 0..queue.GetNumStoredMessages() {
                    let mut size = 0;
                    queue.GetMessage(i, None, &mut size).unwrap();
                    let mut storage = vec![0usize; size.div_ceil(size_of::<usize>())];
                    let message = storage.as_mut_ptr().cast::<D3D12_MESSAGE>();
                    queue.GetMessage(i, Some(message), &mut size).unwrap();
                    assert!(
                        (*message).Severity != D3D12_MESSAGE_SEVERITY_ERROR
                            && (*message).Severity != D3D12_MESSAGE_SEVERITY_CORRUPTION,
                        "D3D12 validation: {}",
                        std::ffi::CStr::from_ptr((*message).pDescription.cast()).to_string_lossy()
                    );
                }
            }
        }
    }
}
