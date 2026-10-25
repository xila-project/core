//! Keep configured metadata buffers coherent across independent native opens.
//!
//! LittleFS already maintains the live object identities through rename,
//! relocation and unlink in `mlist`. Use that list under the filesystem lock;
//! pathname caches would alias replacement entries and require another registry.
use crate::{attributes::InternalAttributes, littlefs};

/// Caller holds the LittleFS filesystem lock; `identity` is an open native file
/// still linked into that filesystem. No Rust reference to another open's
/// metadata may be live across this call (all attribute operations use that lock).
pub(crate) fn synchronize(
    filesystem: &mut littlefs::lfs_t,
    identity: &littlefs::lfs_file_t,
    attributes: &InternalAttributes,
) {
    // Unlinked opens have null pairs. Never group separate unlinked objects by
    // that sentinel; duplicate descriptors already share their Rust context.
    if identity.m.pair.contains(&u32::MAX) {
        return;
    }
    let mut node = filesystem.mlist;
    while !node.is_null() {
        // SAFETY: LittleFS owns the live mlist while the filesystem lock is held.
        // Only REG nodes have lfs_file_t's full layout; the common prefix is
        // provided by LittleFS's generated bindings.
        unsafe {
            let current = &*node;
            let next = current.next;
            if current.type_ as u32 == littlefs::lfs_type_LFS_TYPE_REG
                && current.id == identity.id
                && current
                    .m
                    .pair
                    .iter()
                    .any(|block| identity.m.pair.contains(block))
            {
                let file = &*node.cast::<littlefs::lfs_file_t>();
                let config = &*file.cfg;
                for index in 0..config.attr_count as usize {
                    let attribute = &*config.attrs.add(index);
                    if attribute.type_ == InternalAttributes::IDENTIFIER
                        && attribute.size as usize == size_of::<InternalAttributes>()
                        && !attribute.buffer.is_null()
                    {
                        // Buffers are stable fields of boxed File objects and
                        // were registered with this exact type/size at lookup.
                        attribute
                            .buffer
                            .cast::<InternalAttributes>()
                            .write(attributes.clone());
                    }
                }
            }
            node = next;
        }
    }
}

pub(crate) fn has_other_open(
    filesystem: &littlefs::lfs_t,
    identity: &littlefs::lfs_file_t,
) -> bool {
    let identity_ptr = core::ptr::from_ref(identity).cast::<littlefs::lfs_lfs_mlist>();
    let mut node = filesystem.mlist;
    while !node.is_null() {
        // SAFETY: same filesystem-lock/list-lifetime requirements as synchronize.
        unsafe {
            let current = &*node;
            if !core::ptr::eq(node, identity_ptr)
                && current.type_ as u32 == littlefs::lfs_type_LFS_TYPE_REG
                && current.id == identity.id
                && current
                    .m
                    .pair
                    .iter()
                    .any(|block| identity.m.pair.contains(block))
            {
                return true;
            }
            node = current.next;
        }
    }
    false
}
