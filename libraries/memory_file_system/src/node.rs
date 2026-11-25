use alloc::boxed::Box;
use file_system::{Inode, Kind, Permissions, Time};
use users::{GroupIdentifier, UserIdentifier};

use crate::attributes::NodeAttributes;

pub(crate) const ROOT_NODE_INDEX: usize = 0;

/// Compact in-memory node entry stored inline inside a single `Vec<Option<Node>>`.
///
/// Field ordering avoids padding between 64-bit, 32-bit, 16-bit, and 8-bit fields,
/// while `Box<str>` provides a non-null niche so `Option<Node>` has the same size as `Node`.
#[derive(Debug)]
pub(crate) struct Node {
    pub(crate) inode: Inode,
    pub(crate) creation: Time,
    pub(crate) modification: Time,
    pub(crate) access: Time,
    pub(crate) status: Time,
    pub(crate) name: Box<str>,
    pub(crate) parent: u32,
    pub(crate) generation: u32,
    pub(crate) user: UserIdentifier,
    pub(crate) group: GroupIdentifier,
    pub(crate) permissions: Permissions,
    pub(crate) kind: Kind,
}

impl Node {
    pub(crate) fn new(
        name: Box<str>,
        parent: u32,
        generation: u32,
        kind: Kind,
        permissions: Permissions,
    ) -> Self {
        let attributes = NodeAttributes::new(kind, permissions);
        Self {
            inode: attributes.inode,
            creation: attributes.creation,
            modification: attributes.modification,
            access: attributes.access,
            status: attributes.status,
            name,
            parent,
            generation,
            user: attributes.user,
            group: attributes.group,
            permissions: attributes.permissions,
            kind: attributes.kind,
        }
    }

    pub(crate) fn with_attributes(
        name: Box<str>,
        parent: u32,
        generation: u32,
        attributes: NodeAttributes,
    ) -> Self {
        Self {
            inode: attributes.inode,
            creation: attributes.creation,
            modification: attributes.modification,
            access: attributes.access,
            status: attributes.status,
            name,
            parent,
            generation,
            user: attributes.user,
            group: attributes.group,
            permissions: attributes.permissions,
            kind: attributes.kind,
        }
    }

    pub(crate) fn is_directory(&self) -> bool {
        self.kind == Kind::Directory
    }

    pub(crate) fn to_attributes(&self) -> NodeAttributes {
        NodeAttributes {
            inode: self.inode,
            creation: self.creation,
            modification: self.modification,
            access: self.access,
            status: self.status,
            user: self.user,
            group: self.group,
            permissions: self.permissions,
            kind: self.kind,
        }
    }

    pub(crate) fn set_attributes(&mut self, attributes: &NodeAttributes) {
        self.inode = attributes.inode;
        self.creation = attributes.creation;
        self.modification = attributes.modification;
        self.access = attributes.access;
        self.status = attributes.status;
        self.user = attributes.user;
        self.group = attributes.group;
        self.permissions = attributes.permissions;
        self.kind = attributes.kind;
    }
}
