/// SparseMerkleTree is optimized sparse merkle tree implementation based on [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md).
use std::cmp;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::consts::{PREFIX_EMPTY, PREFIX_LEAF_HASH};
use crate::database::traits::Actions;
use crate::types::{
    ArcMutex, Cache, Hash256, HashKind, HashWithKind, Height, KVPair, KeyLength, NestedVec,
    NestedVecOfSlices, SharedKVPair, SharedNestedVec, SharedVec, StructurePosition, SubtreeHeight,
};
use crate::utils;

/// PREFIX_SUB_TREE_LEAF is for leaf prefix for sub tree.
const PREFIX_SUB_TREE_LEAF: u8 = 0;
/// PREFIX_SUB_TREE_BRANCH is for branch prefix for sub tree.
const PREFIX_SUB_TREE_BRANCH: u8 = 1;
/// PREFIX_SUB_TREE_EMPTY is for empty prefix for sub tree.
const PREFIX_SUB_TREE_EMPTY: u8 = 2;
/// Hash size used in the smt.
const HASH_SIZE: usize = 32;
/// EMPTY_HASH using sha256.
pub const EMPTY_HASH: [u8; 32] = [
    227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36, 39, 174, 65, 228,
    100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85,
];

type SharedNode = ArcMutex<Node>;

trait SortDescending {
    fn sort_descending(&mut self);
}

#[derive(Error, Debug, PartialEq, Eq)]
pub enum SMTError {
    #[error("Invalid bitmap length")]
    InvalidBitmapLen,
    #[error("Invalid input: `{0}`")]
    InvalidInput(String),
    #[error("unknown data not found error `{0}`")]
    NotFound(String),
    #[error("Invalid state root `{0}`")]
    InvalidRoot(String),
    #[error("unknown data store error `{0}`")]
    Unknown(String),
}

#[derive(Clone, Debug, PartialEq)]
enum NodeKind {
    Empty,
    Leaf,
    Stub, // stub is a branch node for sub tree.
    Temp, // temp is a stub but only used during the calculation.
}

/// UpdateData holds key-value pairs to update the SMT.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateData {
    data: Cache,
}

/// Proof holds SMT proof.
#[derive(Clone, Debug)]
pub struct Proof {
    pub sibling_hashes: NestedVec,
    pub queries: Vec<QueryProof>,
}

/// QueryProof is single proof for a query.
#[derive(Clone, Debug)]
pub struct QueryProof {
    pub pair: Arc<KVPair>,
    pub bitmap: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
pub struct QueryProofWithProof {
    query_proof: QueryProof,
    binary_bitmap: Vec<bool>,
    ancestor_hashes: NestedVec,
    sibling_hashes: NestedVec,
    hash: Vec<u8>,
    is_removed: bool,
}

#[derive(Clone, Debug)]
struct Node {
    kind: NodeKind,
    // key is represented of the Data and value is represented of the Hash
    hash: KVPair,
    key: Vec<u8>,
    index: usize,
}

#[derive(Clone, Debug)]
struct SubTree {
    structure: Vec<u8>,
    nodes: Vec<SharedNode>,
    root: Arc<Vec<u8>>,
}

struct GenerateResultData<'a> {
    query_key: &'a [u8],
    current_node: &'a Node,
    query_hashes: QueryHashes<'a>,
    height: Height,
    query_height: Height,
}

struct QueryHashes<'a> {
    ancestor_hashes: &'a mut VecDeque<Vec<u8>>,
    sibling_hashes: &'a mut VecDeque<Vec<u8>>,
    binary_bitmap: &'a mut Vec<bool>,
}

struct QueryHashesInfo<'a> {
    layer_nodes: Vec<SharedNode>,
    layer_structure: Vec<u8>,
    ref_mut_vecs: QueryHashes<'a>,
    extra: QueryHashesExtraInfo,
}

struct QueryHashesExtraInfo {
    height: Height,
    target_id: usize,
    max_index: usize,
}

struct NextQueryHashesInfo {
    layer_nodes: Vec<SharedNode>,
    layer_structure: Vec<u8>,
    target_id: usize,
}
struct UpdateNodeInfo<'a> {
    key_bins: &'a [SharedNestedVec<'a>],
    value_bins: &'a [SharedNestedVec<'a>],
    length_bins: &'a [u32],
    current_node: SharedNode,
    length_base: u32,
    height: Height,
    structure_pos: StructurePosition,
}

struct Bins<'a> {
    keys: Vec<SharedNestedVec<'a>>,
    values: Vec<SharedNestedVec<'a>>,
}

struct UpdatedInfo {
    nodes: Vec<SharedNode>,
    structures: Vec<u8>,
    bin_offset: usize,
}

/// SparseMerkleTree is optimized sparse merkle tree implementation based on [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md).
pub struct SparseMerkleTree {
    root: SharedVec,
    key_length: KeyLength,
    /// key_length specifies the length of the key for the SMT. All the keys must follow this length.
    subtree_height: SubtreeHeight,
    /// height of the sub tree. Increase in the subtree height will increase number of hashes used while it decreases call to the storage.
    max_number_of_nodes: usize,
}

#[derive(Clone)]
struct Hasher {
    node_hashes: Vec<Arc<Vec<u8>>>,
    structure: Vec<u8>,
    height: Height,
}

impl Hash256 for KVPair {
    fn hash(&self) -> Vec<u8> {
        let mut hasher = Sha256::new();
        hasher.update(PREFIX_LEAF_HASH);
        hasher.update(self.key());
        hasher.update(self.value());
        let result = hasher.finalize();
        result.to_vec()
    }
}

fn get_parent_node(
    layer_nodes: &[SharedNode],
    layer_structure: &[u8],
    tree_map: &mut VecDeque<(Vec<SharedNode>, Vec<u8>)>,
    i: usize,
) -> Result<SharedNode, SMTError> {
    let layer_node_kind = layer_nodes[i].lock().unwrap().kind.clone();
    let layer_node_next_kind = layer_nodes[i + 1].lock().unwrap().kind.clone();

    if layer_node_kind == NodeKind::Empty && layer_node_next_kind == NodeKind::Empty {
        Ok(Arc::clone(&layer_nodes[i]))
    } else if layer_node_kind == NodeKind::Empty && layer_node_next_kind == NodeKind::Leaf {
        Ok(Arc::clone(&layer_nodes[i + 1]))
    } else if layer_node_kind == NodeKind::Leaf && layer_node_next_kind == NodeKind::Empty {
        Ok(Arc::clone(&layer_nodes[i]))
    } else {
        let (mut left_nodes, mut left_structure) = if layer_node_kind == NodeKind::Temp {
            let (nodes, structure) = tree_map
                .pop_back()
                .ok_or_else(|| SMTError::Unknown(String::from("Subtree must exist for stub")))?;
            (nodes, structure)
        } else {
            (vec![Arc::clone(&layer_nodes[i])], vec![layer_structure[i]])
        };
        let (right_nodes, right_structure) = if layer_node_next_kind == NodeKind::Temp {
            let (nodes, structure) = tree_map
                .pop_back()
                .ok_or_else(|| SMTError::Unknown(String::from("Subtree must exist for stub")))?;
            (nodes, structure)
        } else {
            (
                vec![Arc::clone(&layer_nodes[i + 1])],
                vec![layer_structure[i + 1]],
            )
        };
        left_structure.extend(right_structure);
        left_nodes.extend(right_nodes);
        let stub = Node::new_temp();
        tree_map.push_front((left_nodes, left_structure));

        Ok(Arc::new(Mutex::new(stub)))
    }
}

fn calculate_subtree(
    layer_nodes: &[SharedNode],
    layer_structure: &[u8],
    height: Height,
    tree_map: &mut VecDeque<(Vec<SharedNode>, Vec<u8>)>,
) -> Result<SubTree, SMTError> {
    let mut layer_nodes = layer_nodes.to_vec();
    let mut layer_structure = layer_structure.to_vec();
    let mut height = height;

    while !height.is_equal_to(0) {
        let mut next_layer_nodes: Vec<SharedNode> = Vec::with_capacity(layer_nodes.len());
        let mut next_layer_structure: Vec<u8> = Vec::with_capacity(layer_nodes.len());
        let mut i = 0;
        while i < layer_nodes.len() {
            if layer_structure[i] != height.into() {
                next_layer_nodes.push(Arc::clone(&layer_nodes[i]));
                next_layer_structure.push(layer_structure[i]);
                i += 1;
                continue;
            }

            let parent = get_parent_node(&layer_nodes, &layer_structure, tree_map, i)?;
            next_layer_nodes.push(parent);
            next_layer_structure.push(layer_structure[i] - 1);
            // using 2 layer nodes
            i += 2;
        }
        if height.is_equal_to(1) {
            if next_layer_nodes[0].lock().unwrap().kind == NodeKind::Temp {
                let (nodes, structure) = tree_map.pop_front().ok_or_else(|| {
                    SMTError::Unknown(String::from("Subtree must exist for stub"))
                })?;
                return SubTree::from_data(&structure, &nodes);
            }
            return SubTree::from_data(&[0], &next_layer_nodes);
        }
        layer_nodes = next_layer_nodes;
        layer_structure = next_layer_structure;
        height = height - Height(1);
    }

    SubTree::from_data(&[0], &layer_nodes)
}

fn calculate_next_info(info: &mut QueryHashesInfo, next_info: &mut NextQueryHashesInfo, i: usize) {
    let layer_node = info.layer_nodes[i].lock().unwrap();
    let layer_node_next = info.layer_nodes[i + 1].lock().unwrap();

    let mut parent_node = Node::new_branch(layer_node.hash.value(), layer_node_next.hash.value());
    parent_node.index = info.extra.max_index + i;
    let parent_node_index = parent_node.index;
    let parent_node_hash = parent_node.hash.value_as_vec();

    next_info
        .layer_nodes
        .push(Arc::new(Mutex::new(parent_node)));
    next_info.layer_structure.push(info.layer_structure[i] - 1);
    if next_info.target_id == layer_node.index {
        info.ref_mut_vecs
            .ancestor_hashes
            .push_front(parent_node_hash);
        next_info.target_id = parent_node_index;
        if layer_node_next.kind == NodeKind::Empty {
            info.ref_mut_vecs.binary_bitmap.push(false);
        } else {
            info.ref_mut_vecs.binary_bitmap.push(true);
            info.ref_mut_vecs
                .sibling_hashes
                .push_front(layer_node_next.hash.value_as_vec());
        }
    } else if next_info.target_id == layer_node_next.index {
        info.ref_mut_vecs
            .ancestor_hashes
            .push_front(parent_node_hash);
        next_info.target_id = parent_node_index;
        if layer_node.kind == NodeKind::Empty {
            info.ref_mut_vecs.binary_bitmap.push(false);
        } else {
            info.ref_mut_vecs.binary_bitmap.push(true);
            info.ref_mut_vecs
                .sibling_hashes
                .push_front(layer_node.hash.value_as_vec());
        }
    }
}

fn calculate_query_hashes(mut info: QueryHashesInfo) {
    let mut is_extra_height_zero = info.extra.height.is_equal_to(0);
    while !is_extra_height_zero {
        let mut next_info = NextQueryHashesInfo::new(info.extra.target_id);
        let mut i = 0;
        while i < info.layer_nodes.len() {
            if info.layer_structure[i] != info.extra.height.into() {
                next_info.push(Arc::clone(&info.layer_nodes[i]), info.layer_structure[i]);
                i += 1;
                continue;
            }
            calculate_next_info(&mut info, &mut next_info, i);
            i += 2;
        }
        let new_extra = QueryHashesExtraInfo::new(
            info.extra.height - Height(1),
            next_info.target_id,
            info.extra.max_index + i + 1,
        );
        info = QueryHashesInfo::new(
            next_info.layer_nodes,
            next_info.layer_structure,
            info.ref_mut_vecs,
            new_extra,
        );
        is_extra_height_zero = info.extra.height.is_equal_to(0);
    }
}

fn insert_and_filter_queries(q: QueryProofWithProof, queries: &mut VecDeque<QueryProofWithProof>) {
    if queries.is_empty() {
        queries.push_back(q);
        return;
    }

    let index = utils::binary_search(queries.make_contiguous(), |val| {
        (q.height() == val.height()
            && utils::compare(q.query_proof.key(), val.query_proof.key()) == cmp::Ordering::Less)
            || q.height() > val.height()
    });

    if index == queries.len() as i32 {
        queries.push_back(q);
        return;
    }

    let original = &queries[index as usize];
    if !utils::array_equal_bool(&q.binary_path(), &original.binary_path()) {
        queries.insert(index as usize, q);
    }
}

fn calculate_sibling_hashes(
    query_with_proofs: &mut VecDeque<QueryProofWithProof>,
    ancestor_hashes: &[Vec<u8>],
    sibling_hashes: &mut NestedVec,
) {
    if query_with_proofs.is_empty() {
        return;
    }
    while !query_with_proofs.is_empty() {
        let mut query = query_with_proofs.pop_front().unwrap();
        if query.is_zero_height() {
            continue;
        }
        if query.binary_bitmap[0] {
            let node_hash = query.sibling_hashes.pop().unwrap();
            if !utils::bytes_in(ancestor_hashes, &node_hash)
                && !utils::bytes_in(sibling_hashes, &node_hash)
            {
                sibling_hashes.push(node_hash);
            }
        }
        query.slice_bitmap();
        insert_and_filter_queries(query, query_with_proofs);
    }
}

impl SortDescending for [QueryProofWithProof] {
    fn sort_descending(&mut self) {
        self.sort_by(|a, b| match a.height().cmp(&b.height()) {
            cmp::Ordering::Greater => cmp::Ordering::Less,
            cmp::Ordering::Less => cmp::Ordering::Greater,
            _ => utils::compare(a.query_proof.key(), b.query_proof.key()),
        });
    }
}

impl std::convert::From<&SMTError> for SMTError {
    fn from(err: &SMTError) -> Self {
        SMTError::Unknown(err.to_string())
    }
}

impl rocksdb::WriteBatchIterator for UpdateData {
    /// Called with a key and value that were `put` into the batch.
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.data.insert(
            key.into_vec().hash_with_kind(HashKind::Key),
            value.into_vec().hash_with_kind(HashKind::Value),
        );
    }
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>) {
        self.data
            .insert(key.into_vec().hash_with_kind(HashKind::Key), vec![]);
    }
}

impl Hasher {
    fn new(node_hashes: &[Arc<Vec<u8>>], structure: &[u8], height: Height) -> Self {
        Self {
            node_hashes: node_hashes.to_vec(),
            structure: structure.to_vec(),
            height,
        }
    }

    fn execute(&mut self) -> Arc<Vec<u8>> {
        while self.node_hashes.len() != 1 {
            let mut next_hashes: Vec<Arc<Vec<u8>>> = Vec::with_capacity(self.node_hashes.len());
            let mut next_structure: Vec<u8> = Vec::with_capacity(self.node_hashes.len());
            let mut i = 0;

            while i < self.node_hashes.len() {
                if self.structure[i] == self.height.into() {
                    let branch = [
                        (*self.node_hashes[i]).as_slice(),
                        (*self.node_hashes[i + 1]).as_slice(),
                    ]
                    .concat();
                    let hash = branch.hash_with_kind(HashKind::Branch);
                    next_hashes.push(Arc::new(hash.to_vec()));
                    next_structure.push(self.structure[i] - 1);
                    i += 1;
                } else {
                    next_hashes.push(Arc::clone(&self.node_hashes[i]));
                    next_structure.push(self.structure[i]);
                }
                i += 1;
            }

            if self.height.is_equal_to(1) {
                return Arc::clone(&next_hashes[0]);
            }

            self.height = self.height - Height(1);
            self.node_hashes = next_hashes;
            self.structure = next_structure;
        }

        Arc::clone(&self.node_hashes[0])
    }
}

impl QueryProof {
    #[inline]
    pub fn new_with_binary_bitmap(pair: Arc<KVPair>, binary_bitmap: &[bool]) -> Self {
        Self {
            pair,
            bitmap: Arc::new(utils::bools_to_bytes(binary_bitmap)),
        }
    }

    #[inline]
    pub fn key(&self) -> &[u8] {
        self.pair.key()
    }

    #[inline]
    pub fn value(&self) -> &[u8] {
        self.pair.value()
    }

    #[inline]
    pub fn key_as_vec(&self) -> Vec<u8> {
        self.pair.key_as_vec()
    }

    #[inline]
    pub fn value_as_vec(&self) -> Vec<u8> {
        self.pair.value_as_vec()
    }
}

impl UpdateData {
    pub fn new_from(data: Cache) -> Self {
        Self { data }
    }

    pub fn insert(&mut self, kv: SharedKVPair) {
        self.data.insert(kv.key_as_vec(), kv.value_as_vec());
    }

    pub fn entries(&self) -> (SharedNestedVec, SharedNestedVec) {
        let mut kv_pair: Vec<SharedKVPair> =
            self.data.iter().map(|(k, v)| SharedKVPair(k, v)).collect();
        kv_pair.sort_by(|a, b| a.0.cmp(b.0));
        let mut keys = Vec::with_capacity(kv_pair.len());
        let mut values = Vec::with_capacity(kv_pair.len());
        for kv in kv_pair {
            keys.push(kv.0);
            values.push(kv.1);
        }
        (keys, values)
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

impl QueryProofWithProof {
    fn new_with_pair(
        pair: Arc<KVPair>,
        binary_bitmap: &[bool],
        ancestor_hashes: &[Vec<u8>],
        sibling_hashes: &[Vec<u8>],
    ) -> Self {
        let hashed_key = if pair.is_empty_value() {
            EMPTY_HASH.to_vec()
        } else {
            pair.hash()
        };
        Self {
            query_proof: QueryProof::new_with_binary_bitmap(pair, binary_bitmap),
            binary_bitmap: binary_bitmap.to_vec(),
            ancestor_hashes: ancestor_hashes.to_vec(),
            sibling_hashes: sibling_hashes.to_vec(),
            hash: hashed_key,
            is_removed: false,
        }
    }
    fn height(&self) -> usize {
        self.binary_bitmap.len()
    }

    fn is_zero_height(&self) -> bool {
        self.binary_bitmap.is_empty()
    }

    fn slice_bitmap(&mut self) {
        self.binary_bitmap = self.binary_bitmap[1..].to_vec();
    }

    fn binary_path(&self) -> Vec<bool> {
        let mut binary_path = utils::bytes_to_bools(self.query_proof.key());
        binary_path.truncate(self.height());
        binary_path
    }

    fn binary_key(&self) -> Vec<bool> {
        utils::bytes_to_bools(self.query_proof.key())
    }

    fn is_sibling_of(&self, query: &QueryProofWithProof) -> bool {
        if self.binary_bitmap.len() != query.binary_bitmap.len() {
            return false;
        }

        // end of bool is exclusive
        if !utils::is_bools_equal(
            &self.binary_key()[..self.height() - 1],
            &query.binary_key()[..query.height() - 1],
        ) {
            return false;
        }

        if !self.binary_key()[self.height() - 1] && query.binary_key()[self.height() - 1] {
            return true;
        }

        if self.binary_key()[self.height() - 1] && !query.binary_key()[self.height() - 1] {
            return true;
        }
        false
    }
}

impl Node {
    fn new_temp() -> Self {
        Self {
            kind: NodeKind::Temp,
            hash: KVPair::new(&[], &[]),
            key: vec![],
            index: 0,
        }
    }

    fn new_stub(node_hash: &[u8]) -> Self {
        let data = [&[PREFIX_SUB_TREE_BRANCH], node_hash].concat();
        Self {
            kind: NodeKind::Stub,
            hash: KVPair::new(&data, node_hash),
            key: vec![],
            index: 0,
        }
    }

    fn new_branch(left_hash: &[u8], right_hash: &[u8]) -> Self {
        let combined = [left_hash, right_hash].concat();
        let data = [&[PREFIX_SUB_TREE_BRANCH], left_hash, right_hash].concat();
        let hashed = combined.hash_with_kind(HashKind::Branch);
        Self {
            kind: NodeKind::Stub,
            hash: KVPair::new(&data, &hashed),
            key: vec![],
            index: 0,
        }
    }

    fn new_leaf(pair: &KVPair) -> Self {
        let h = pair.hash();
        let data = [&[PREFIX_SUB_TREE_LEAF], pair.key(), pair.value()].concat();
        Self {
            kind: NodeKind::Leaf,
            hash: KVPair::new(&data, &h),
            key: pair.key_as_vec(),
            index: 0,
        }
    }

    fn new_empty() -> Self {
        let data = [PREFIX_EMPTY].concat();
        Self {
            kind: NodeKind::Empty,
            hash: KVPair::new(&data, &EMPTY_HASH),
            key: vec![],
            index: 0,
        }
    }
}

impl SubTree {
    /// new returns decoded SubTree using the encoded data.
    pub fn new(data: &[u8], key_length: KeyLength) -> Result<Self, SMTError> {
        if data.is_empty() {
            return Err(SMTError::InvalidInput(String::from("keys length is zero")));
        }
        let node_length: usize = data[0] as usize + 1;
        let structure = &data[1..node_length + 1];
        let node_data = &data[node_length + 1..];
        let mut nodes: Vec<SharedNode> = Vec::with_capacity(node_data.len());
        let mut idx = 0;

        let key_length: usize = key_length.into();
        while idx < node_data.len() {
            match node_data[idx] {
                PREFIX_SUB_TREE_LEAF => {
                    let kv = KVPair::new(
                        &node_data[idx + [PREFIX_SUB_TREE_LEAF].len()
                            ..idx + [PREFIX_SUB_TREE_LEAF].len() + key_length],
                        &node_data[idx + [PREFIX_SUB_TREE_LEAF].len() + key_length
                            ..idx + [PREFIX_SUB_TREE_LEAF].len() + key_length + HASH_SIZE],
                    );
                    let node = Node::new_leaf(&kv);
                    nodes.push(Arc::new(Mutex::new(node)));
                    idx += [PREFIX_SUB_TREE_LEAF].len() + key_length + HASH_SIZE;
                },
                PREFIX_SUB_TREE_BRANCH => {
                    let node_hash = &node_data[idx + [PREFIX_SUB_TREE_BRANCH].len()
                        ..idx + [PREFIX_SUB_TREE_BRANCH].len() + HASH_SIZE];
                    nodes.push(Arc::new(Mutex::new(Node::new_stub(node_hash))));
                    idx += [PREFIX_SUB_TREE_BRANCH].len() + HASH_SIZE;
                },
                PREFIX_SUB_TREE_EMPTY => {
                    nodes.push(Arc::new(Mutex::new(Node::new_empty())));
                    idx += PREFIX_EMPTY.len();
                },
                _ => {
                    return Err(SMTError::InvalidInput(String::from(
                        "Invalid data. key prefix is invalid.",
                    )));
                },
            }
        }

        SubTree::from_data(structure, &nodes)
    }

    /// from_data creates SubTree from structure and nodes information.
    pub fn from_data(structure: &[u8], nodes: &[SharedNode]) -> Result<Self, SMTError> {
        let height: Height = structure
            .iter()
            .max()
            .ok_or_else(|| SMTError::Unknown(String::from("Invalid structure")))?
            .into();

        let node_hashes = nodes
            .iter()
            .map(|n| Arc::new(n.lock().unwrap().hash.value_as_vec()))
            .collect::<Vec<Arc<Vec<u8>>>>();
        let mut hasher = Hasher::new(&node_hashes, structure, height);
        let calculated = hasher.execute();

        Ok(Self {
            structure: structure.to_vec(),
            nodes: nodes.to_vec(),
            root: calculated,
        })
    }

    /// new_empty returns empty SubTree.
    pub fn new_empty() -> Self {
        let structure = vec![0];
        let empty = Node::new_empty();
        let node_hashes = vec![Arc::new(Mutex::new(Node::new_empty()))];

        Self {
            structure,
            nodes: node_hashes,
            root: Arc::new(empty.hash.value_as_vec()),
        }
    }

    /// encode SubTree into bytes slice, which can be used in "new".
    pub fn encode(&self) -> Vec<u8> {
        let node_length = (self.structure.len() - 1) as u8;
        let node_hashes: NestedVec = self
            .nodes
            .iter()
            .map(|n| n.lock().unwrap().hash.key_as_vec())
            .collect();
        [
            &[node_length],
            self.structure.as_slice(),
            node_hashes.concat().as_slice(),
        ]
        .concat()
    }
}

impl QueryHashesExtraInfo {
    fn new(height: Height, target_id: usize, max_index: usize) -> Self {
        Self {
            height,
            target_id,
            max_index,
        }
    }
}

impl<'a> QueryHashesInfo<'a> {
    fn new(
        layer_nodes: Vec<SharedNode>,
        layer_structure: Vec<u8>,
        vecs: QueryHashes<'a>,
        extra: QueryHashesExtraInfo,
    ) -> Self {
        QueryHashesInfo {
            layer_nodes,
            layer_structure,
            ref_mut_vecs: QueryHashes {
                sibling_hashes: vecs.sibling_hashes,
                ancestor_hashes: vecs.ancestor_hashes,
                binary_bitmap: vecs.binary_bitmap,
            },
            extra,
        }
    }
}

impl NextQueryHashesInfo {
    fn new(target_id: usize) -> Self {
        NextQueryHashesInfo {
            layer_nodes: vec![],
            layer_structure: vec![],
            target_id,
        }
    }

    fn push(&mut self, old_layer_nodes: SharedNode, old_layer_structure: u8) {
        self.layer_nodes.push(old_layer_nodes);
        self.layer_structure.push(old_layer_structure);
    }
}

impl<'a> UpdateNodeInfo<'a> {
    fn new(
        key_bins: &'a [SharedNestedVec],
        value_bins: &'a [SharedNestedVec],
        length_bins: &'a [u32],
        current_node: SharedNode,
        length_base: u32,
        height: Height,
        structure_pos: StructurePosition,
    ) -> Self {
        Self {
            key_bins,
            value_bins,
            length_bins,
            current_node,
            length_base,
            height,
            structure_pos,
        }
    }
}

impl SparseMerkleTree {
    fn get_proof_queries(&self, query_with_proofs: &[QueryProofWithProof]) -> Vec<QueryProof> {
        let proof_queries: Vec<QueryProof> = query_with_proofs
            .iter()
            .map(|query| QueryProof {
                pair: Arc::clone(&query.query_proof.pair),
                bitmap: Arc::clone(&query.query_proof.bitmap),
            })
            .collect();

        proof_queries
    }

    fn generate_sibling_data(
        &mut self,
        db: &mut impl Actions,
        queries: &[Vec<u8>],
    ) -> Result<(Vec<QueryProofWithProof>, NestedVec), SMTError> {
        let mut query_with_proofs: Vec<QueryProofWithProof> = Vec::with_capacity(queries.len());
        let mut root = self.get_subtree(db, &self.root.lock().unwrap())?;
        let mut ancestor_hashes = Vec::with_capacity(queries.len());
        for query in queries {
            let query_proof = self.generate_query_proof(db, &mut root, query, Height(0))?;
            query_with_proofs.push(query_proof.clone());
            ancestor_hashes.extend(query_proof.ancestor_hashes);
        }

        Ok((query_with_proofs, ancestor_hashes))
    }

    /// verify_and_prepare_proof_map checks all verifications of query_keys based on the verify function in the [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md#proof-construction).
    fn verify_and_prepare_proof_map(
        proof: &Proof,
        query_keys: &[Vec<u8>],
        key_length: KeyLength,
    ) -> Result<Vec<QueryProofWithProof>, SMTError> {
        if query_keys.len() != proof.queries.len() {
            return Err(SMTError::InvalidInput(String::from(
                "Mismatched length of keys and the queries of the proof",
            )));
        }
        let mut queries_with_proof: Vec<QueryProofWithProof> = vec![];
        let mut queries: HashMap<Vec<bool>, QueryProof> = HashMap::new();
        for (i, key) in query_keys.iter().enumerate() {
            // Check if all the query keys have the same length.
            if proof.queries[i].key().len() != key_length.into() || key.len() != key_length.into()
            {
                return Err(SMTError::InvalidInput(String::from(
                    "The length of the key is invalid",
                )));
            }

            let query = &proof.queries[i];
            if query.bitmap.len() > 0 && query.bitmap[0] == 0 {
                return Err(SMTError::InvalidBitmapLen);
            }

            let binary_bitmap = utils::strip_left_false(&utils::bytes_to_bools(&query.bitmap));
            let query_key_binary = utils::bytes_to_bools(query.key());
            if !utils::is_bytes_equal(key, query.key()) {
                if query.value().is_empty() {
                    return Err(SMTError::InvalidInput(String::from(
                        "Proof for which query key and proof key differs, must have a non-empty value",
                    )));
                }

                let key_binary = utils::bytes_to_bools(key);
                let common_prefix = utils::common_prefix(&key_binary, &query_key_binary);
                if binary_bitmap.len() > common_prefix.len() {
                    return Err(SMTError::InvalidBitmapLen);
                }
            }
            let binary_path = if binary_bitmap.len() > query_key_binary.len() {
                return Err(SMTError::InvalidBitmapLen);
            } else {
                query_key_binary[..binary_bitmap.len()].to_vec()
            };
            if let Some(duplicate_query) = queries.get(&binary_path) {
                if !utils::is_bytes_equal(&duplicate_query.bitmap, &query.bitmap)
                    || !utils::is_bytes_equal(duplicate_query.value(), query.value())
                {
                    return Err(SMTError::InvalidInput(String::from(
                        "Mismatched values or bitmap",
                    )));
                }

                if !query.value().is_empty()
                    && !utils::is_bytes_equal(query.key(), duplicate_query.key())
                {
                    return Err(SMTError::InvalidInput(String::from("Mismatched keys")));
                }
            } else {
                queries.insert(binary_path.clone(), query.clone());
                queries_with_proof.push(QueryProofWithProof::new_with_pair(
                    Arc::clone(&query.pair),
                    &binary_bitmap,
                    &[],
                    &[],
                ));
            }
        }

        Ok(queries_with_proof)
    }

    /// get_subtree returns sub_tree based on the node_hash provided.
    /// if node_has is empty or equals to the empty hash, it returns empty SubTree.
    fn get_subtree(&self, db: &impl Actions, node_hash: &[u8]) -> Result<SubTree, SMTError> {
        if node_hash.is_empty() {
            return Ok(SubTree::new_empty());
        }

        if utils::is_empty_hash(node_hash) {
            return Ok(SubTree::new_empty());
        }

        let value = db
            .get(node_hash)
            .map_err(|err| SMTError::Unknown(err.to_string()))?
            .ok_or_else(|| SMTError::NotFound(String::from("node_hash does not exist")))?;

        SubTree::new(&value, self.key_length)
    }

    fn calculate_bins<'a>(
        &mut self,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        height: Height,
    ) -> Result<Bins<'a>, SMTError> {
        let mut keys: NestedVecOfSlices = vec![vec![]; self.max_number_of_nodes];
        let mut values: NestedVecOfSlices = vec![vec![]; self.max_number_of_nodes];

        let b = height.div_to_usize(8);
        for i in 0..key_bin.len() {
            let k = key_bin[i];
            let v = value_bin[i];
            let bin_idx = if self.subtree_height.is_four() {
                match height.mod_to_u8(8) {
                    0 => Ok(k[b] >> 4),
                    4 => Ok(k[b] & 15),
                    _ => Err(SMTError::Unknown(String::from("Invalid bin index"))),
                }?
            // when subtree_height is 8
            } else {
                k[b]
            };
            keys[bin_idx as usize].push(k);
            values[bin_idx as usize].push(v);
        }

        Ok(Bins { keys, values })
    }

    /// calculate_updated_info computes the update of SMT using key value pairs.
    fn calculate_updated_info<'a>(
        &mut self,
        db: &mut impl Actions,
        current_subtree: &SubTree,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        height: Height,
    ) -> Result<UpdatedInfo, SMTError> {
        let bins = self.calculate_bins(key_bin, value_bin, height)?;
        let mut nodes: Vec<SharedNode> = vec![];
        let mut structures: Vec<u8> = vec![];
        let mut bin_offset = 0;
        for i in 0..current_subtree.nodes.len() {
            let pos = current_subtree.structure[i];
            let current_node = Arc::clone(&current_subtree.nodes[i]);
            let new_offset = 1 << self.subtree_height.sub_to_usize(pos);

            let slice_keys = &bins.keys[bin_offset..bin_offset + new_offset];
            let slice_values = &bins.values[bin_offset..bin_offset + new_offset];
            let mut sum = 0;
            let base_length: Vec<u32> = slice_keys
                .iter()
                .map(|kb| {
                    sum += kb.len() as u32;
                    sum
                })
                .collect();

            let info = UpdateNodeInfo::new(
                slice_keys,
                slice_values,
                &base_length,
                Arc::clone(&current_node),
                0,
                height,
                pos.into(),
            );
            let (updated_nodes, heights) = self.update_node(db, info)?;

            for node in updated_nodes {
                nodes.push(node);
            }

            structures.extend(heights);
            bin_offset += new_offset;
        }

        Ok(UpdatedInfo {
            nodes,
            structures,
            bin_offset,
        })
    }

    /// update_subtree updates the SubTree based on the keys and values.
    fn update_subtree<'a>(
        &mut self,
        db: &mut impl Actions,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        current_subtree: &SubTree,
        height: Height,
    ) -> Result<SubTree, SMTError> {
        if key_bin.is_empty() {
            return Ok(current_subtree.clone());
        }
        let updated =
            self.calculate_updated_info(db, current_subtree, key_bin, value_bin, height)?;
        if updated.bin_offset != self.max_number_of_nodes {
            return Err(SMTError::Unknown(format!(
                "bin_offset {} expected {}",
                updated.bin_offset, self.max_number_of_nodes
            )));
        }
        // Go through nodes again and push up empty nodes
        let max_structure = updated
            .structures
            .iter()
            .max()
            .ok_or_else(|| SMTError::Unknown(String::from("Invalid structure")))?;
        let mut tree_map = VecDeque::new();

        let new_subtree = calculate_subtree(
            &updated.nodes,
            &updated.structures,
            max_structure.into(),
            &mut tree_map,
        )?;
        let value = new_subtree.encode();
        db.set(&KVPair::new(&new_subtree.root, &value))
            .map_err(|err| SMTError::Unknown(err.to_string()))?;

        Ok(new_subtree)
    }

    /// update_single_node handles node update when info only affects single node.
    fn update_single_node(
        &self,
        info: &UpdateNodeInfo,
    ) -> Result<Option<(SharedNode, StructurePosition)>, SMTError> {
        let idx = info
            .length_bins
            .iter()
            .position(|&r| r == info.length_base + 1)
            .ok_or_else(|| SMTError::Unknown(String::from("Invalid index")))?;

        let current_node = info.current_node.lock().unwrap();

        if current_node.kind == NodeKind::Empty {
            if !info.value_bins[idx][0].is_empty() {
                let new_leaf =
                    Node::new_leaf(&KVPair::new(info.key_bins[idx][0], info.value_bins[idx][0]));
                return Ok(Some((Arc::new(Mutex::new(new_leaf)), info.structure_pos)));
            }
            return Ok(Some((Arc::clone(&info.current_node), info.structure_pos)));
        }

        if current_node.kind == NodeKind::Leaf
            && utils::is_bytes_equal(&current_node.key, info.key_bins[idx][0])
        {
            if !info.value_bins[idx][0].is_empty() {
                let new_leaf =
                    Node::new_leaf(&KVPair::new(info.key_bins[idx][0], info.value_bins[idx][0]));
                return Ok(Some((Arc::new(Mutex::new(new_leaf)), info.structure_pos)));
            }
            return Ok(Some((
                Arc::new(Mutex::new(Node::new_empty())),
                info.structure_pos,
            )));
        }

        Ok(None)
    }

    fn update_same_height(
        &mut self,
        db: &mut impl Actions,
        info: &UpdateNodeInfo,
    ) -> Result<(SharedNode, StructurePosition), SMTError> {
        let current_node_kind = info.current_node.lock().unwrap().kind.clone();
        let btm_subtree = match current_node_kind {
            NodeKind::Stub => {
                let subtree =
                    self.get_subtree(db, info.current_node.lock().unwrap().hash.value())?;
                db.del(info.current_node.lock().unwrap().hash.value())
                    .map_err(|err| SMTError::Unknown(err.to_string()))?;
                subtree
            },
            NodeKind::Empty => {
                self.get_subtree(db, info.current_node.lock().unwrap().hash.value())?
            },
            NodeKind::Leaf => SubTree::from_data(&[0], &[Arc::clone(&info.current_node)])?,
            _ => {
                return Err(SMTError::Unknown(String::from("invalid node type")));
            },
        };
        if info.key_bins.len() != 1 || info.value_bins.len() != 1 {
            return Err(SMTError::Unknown(String::from("invalid key/value length")));
        }
        let new_subtree = self.update_subtree(
            db,
            &info.key_bins[0],
            &info.value_bins[0],
            &btm_subtree,
            info.height + info.structure_pos.into(),
        )?;
        if new_subtree.nodes.len() == 1 {
            return Ok((Arc::clone(&new_subtree.nodes[0]), info.structure_pos));
        }
        let new_branch = Node::new_stub(&new_subtree.root);

        Ok((Arc::new(Mutex::new(new_branch)), info.structure_pos))
    }

    fn get_left_and_right_nodes(
        &self,
        info: &UpdateNodeInfo,
    ) -> Result<(SharedNode, SharedNode), SMTError> {
        let current_node = info.current_node.lock().unwrap();

        match current_node.kind {
            NodeKind::Empty => Ok((
                Arc::new(Mutex::new(Node::new_empty())),
                Arc::new(Mutex::new(Node::new_empty())),
            )),
            NodeKind::Leaf => {
                if utils::is_bit_set(
                    &current_node.key,
                    (info.height + info.structure_pos.into()).into(),
                ) {
                    Ok((
                        Arc::new(Mutex::new(Node::new_empty())),
                        Arc::clone(&info.current_node),
                    ))
                } else {
                    Ok((
                        Arc::clone(&info.current_node),
                        Arc::new(Mutex::new(Node::new_empty())),
                    ))
                }
            },
            _ => Err(SMTError::Unknown(String::from("Invalid node kind"))),
        }
    }

    /// update_node updates all the nodes below.
    fn update_node(
        &mut self,
        db: &mut impl Actions,
        info: UpdateNodeInfo,
    ) -> Result<(Vec<SharedNode>, Vec<u8>), SMTError> {
        let total_data = info.length_bins[info.length_bins.len() - 1] - info.length_base;
        // current node is the bottom
        if total_data == 0 {
            return Ok((
                vec![Arc::clone(&info.current_node)],
                vec![info.structure_pos.into()],
            ));
        }
        // remaining data only has one side. Update the node and complete
        if total_data == 1 {
            if let Some((node, structure)) = self.update_single_node(&info)? {
                return Ok((vec![node], vec![structure.into()]));
            }
        }

        if info.structure_pos == self.subtree_height.into() {
            let (node, structure) = self.update_same_height(db, &info)?;
            return Ok((vec![node], vec![structure.into()]));
        }

        // Update left side of the node recursively
        let (left_node, right_node) = self.get_left_and_right_nodes(&info)?;
        let idx = info.key_bins.len() / 2;
        let left_info = UpdateNodeInfo::new(
            &info.key_bins[0..idx],
            &info.value_bins[0..idx],
            &info.length_bins[0..idx],
            left_node,
            info.length_base,
            info.height,
            info.structure_pos + StructurePosition(1),
        );
        let (mut left_nodes, mut left_heights) = self.update_node(db, left_info)?;
        // Update right side of the node recursively
        let right_info = UpdateNodeInfo::new(
            &info.key_bins[idx..],
            &info.value_bins[idx..],
            &info.length_bins[idx..],
            right_node,
            info.length_bins[idx - 1],
            info.height,
            info.structure_pos + StructurePosition(1),
        );
        let (right_nodes, right_heights) = self.update_node(db, right_info)?;

        left_nodes.extend(right_nodes);
        left_heights.extend(right_heights);

        Ok((left_nodes, left_heights))
    }

    fn find_index(&mut self, query_key: &[u8], height: Height) -> Result<u8, SMTError> {
        let b = height.div_to_usize(8);
        if self.subtree_height.is_four() {
            match height.mod_to_u8(8) {
                0 => Ok(query_key[b] >> 4),
                4 => Ok(query_key[b] & 15),
                _ => Err(SMTError::Unknown(String::from("Invalid bin index"))),
            }
        // when subtree_height is 8
        } else {
            Ok(query_key[b])
        }
    }

    fn find_current_node(
        &mut self,
        current_subtree: &SubTree,
        query_key: &[u8],
        height: Height,
    ) -> Result<(SharedNode, Height), SMTError> {
        let mut bin_offset: usize = 0;
        let mut current_node: Option<SharedNode> = None;
        let bin_idx: usize = self.find_index(query_key, height)?.into();
        let mut h = 0;
        for i in 0..current_subtree.nodes.len() {
            h = current_subtree.structure[i];
            current_node = Some(Arc::clone(&current_subtree.nodes[i]));
            let new_offset: usize = 1 << (self.subtree_height.sub_to_usize(h));
            if bin_offset <= bin_idx && bin_idx < bin_offset + new_offset {
                break;
            }
            bin_offset += new_offset
        }

        Ok((Arc::clone(&current_node.unwrap()), h.into()))
    }

    fn calculate_query_proof_from_result(
        &mut self,
        db: &mut impl Actions,
        d: &GenerateResultData,
    ) -> Result<QueryProofWithProof, SMTError> {
        let mut ancestor_hashes = d.query_hashes.ancestor_hashes.clone();
        let sibling_hashes = Vec::from(d.query_hashes.sibling_hashes.clone());
        let binary_bitmap = d.query_hashes.binary_bitmap.clone();
        if d.current_node.kind == NodeKind::Empty {
            let pair = Arc::new(KVPair::new(d.query_key, &[]));
            return Ok(QueryProofWithProof::new_with_pair(
                pair,
                d.query_hashes.binary_bitmap,
                &Vec::from(ancestor_hashes),
                &(sibling_hashes),
            ));
        }

        if d.current_node.kind == NodeKind::Leaf {
            ancestor_hashes.push_back(d.current_node.hash.value_as_vec());
            let key_length: usize = self.key_length.into();
            let pair = Arc::new(KVPair::new(
                &d.current_node.key,
                &d.current_node.hash.key()[[PREFIX_SUB_TREE_LEAF].len() + key_length..],
            ));
            return Ok(QueryProofWithProof::new_with_pair(
                pair,
                // 0 index is the leaf prefix
                &binary_bitmap,
                &Vec::from(ancestor_hashes),
                &sibling_hashes,
            ));
        }

        let mut lower_subtree = self.get_subtree(db, d.current_node.hash.value())?;
        let lower_query_proof = self.generate_query_proof(
            db,
            &mut lower_subtree,
            d.query_key,
            d.height + d.query_height,
        )?;

        let combined_binary_bitmap = [lower_query_proof.binary_bitmap, binary_bitmap].concat();
        Ok(QueryProofWithProof::new_with_pair(
            lower_query_proof.query_proof.pair,
            &combined_binary_bitmap,
            &[
                Vec::from(ancestor_hashes),
                lower_query_proof.ancestor_hashes,
            ]
            .concat(),
            &[sibling_hashes, lower_query_proof.sibling_hashes].concat(),
        ))
    }

    fn calculate_query_hashes_extra_info(
        &self,
        current_subtree: &mut SubTree,
        current_node: &Node,
    ) -> Result<QueryHashesExtraInfo, SMTError> {
        let max_structure = current_subtree
            .structure
            .iter()
            .max()
            .ok_or_else(|| SMTError::Unknown(String::from("Invalid structure")))?;

        Ok(QueryHashesExtraInfo::new(
            max_structure.into(),
            current_node.index,
            current_subtree.nodes.len(),
        ))
    }

    /// generate_query_proof creates proof for single query according to the [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md#proof-construction).
    fn generate_query_proof(
        &mut self,
        db: &mut impl Actions,
        current_subtree: &mut SubTree,
        query_key: &[u8],
        height: Height,
    ) -> Result<QueryProofWithProof, SMTError> {
        if query_key.len() != self.key_length.into() {
            return Err(SMTError::InvalidInput(String::from(
                "Query key length must be equal to key length",
            )));
        }

        for (i, node) in current_subtree.nodes.iter_mut().enumerate() {
            node.lock().unwrap().index = i;
        }

        let (current_node, query_height) =
            self.find_current_node(current_subtree, query_key, height)?;

        let mut ancestor_hashes = VecDeque::new();
        let mut sibling_hashes = VecDeque::new();
        let mut binary_bitmap: Vec<bool> = vec![];
        let extra = self
            .calculate_query_hashes_extra_info(current_subtree, &current_node.lock().unwrap())?;
        let info = QueryHashesInfo::new(
            current_subtree.nodes.clone(),
            current_subtree.structure.clone(),
            QueryHashes {
                ancestor_hashes: &mut ancestor_hashes,
                sibling_hashes: &mut sibling_hashes,
                binary_bitmap: &mut binary_bitmap,
            },
            extra,
        );
        calculate_query_hashes(info);
        let data = GenerateResultData {
            query_key,
            current_node: &current_node.lock().unwrap(),
            query_hashes: QueryHashes {
                ancestor_hashes: &mut ancestor_hashes,
                sibling_hashes: &mut sibling_hashes,
                binary_bitmap: &mut binary_bitmap,
            },
            height,
            query_height,
        };
        self.calculate_query_proof_from_result(db, &data)
    }

    fn next_query(query: &QueryProofWithProof, sibling_hash: &[u8]) -> QueryProofWithProof {
        let d = query.binary_key()[query.height() - 1];
        let mut next_query = query.clone();
        if !d {
            next_query.hash = [query.hash.as_slice(), sibling_hash]
                .concat()
                .hash_with_kind(HashKind::Branch);
        } else {
            next_query.hash = [sibling_hash, query.hash.as_slice()]
                .concat()
                .hash_with_kind(HashKind::Branch);
        }
        next_query.slice_bitmap();

        next_query
    }

    fn is_bitmap_valid(
        sibling: &QueryProofWithProof,
        query: &QueryProofWithProof,
    ) -> Result<bool, SMTError> {
        let is_sibling_empty = utils::is_empty_hash(&sibling.hash);
        if (is_sibling_empty && query.binary_bitmap[0])
            || (!is_sibling_empty && !query.binary_bitmap[0])
        {
            return Err(SMTError::InvalidInput(String::from(
                "bitmap is not consistent with the nodes type",
            )));
        }
        let is_query_empty = utils::is_empty_hash(&query.hash);
        if (is_query_empty && sibling.binary_bitmap[0])
            || (!is_query_empty && !sibling.binary_bitmap[0])
        {
            return Err(SMTError::InvalidInput(String::from(
                "bitmap is not consistent with the nodes type",
            )));
        }
        if !utils::array_equal_bool(&query.binary_bitmap[1..], &sibling.binary_bitmap[1..]) {
            return Err(SMTError::InvalidInput(String::from(
                "nodes do not share common path",
            )));
        }

        Ok(true)
    }

    fn prepare_result_proof(
        proof: &Proof,
        added_sibling_hashes: &[(usize, Vec<u8>)],
        removed_sibling_hashes: &NestedVec,
        removed_keys: &[&[u8]],
        next_sibling_hash_index: usize,
    ) -> Result<Proof, SMTError> {
        if next_sibling_hash_index != proof.sibling_hashes.len() {
            return Err(SMTError::InvalidInput(String::from(
                "Not all sibling hashes were used",
            )));
        }
        let mut sibling_hashes = proof.sibling_hashes.clone();
        for hash in added_sibling_hashes {
            if sibling_hashes.contains(&hash.1) {
                return Err(SMTError::InvalidInput(String::from(
                    "Duplicate sibling hashes",
                )));
            }
            if hash.0 > sibling_hashes.len() {
                sibling_hashes.push(hash.1.clone());
            } else {
                sibling_hashes.insert(hash.0, hash.1.clone());
            }
        }

        let mut updated_sibling_hashes: NestedVec = vec![];
        for hash in sibling_hashes.iter() {
            if !removed_sibling_hashes.contains(hash) {
                updated_sibling_hashes.push(hash.clone());
            }
        }
        if sibling_hashes.len() - updated_sibling_hashes.len() != removed_sibling_hashes.len() {
            return Err(SMTError::InvalidInput(String::from(
                "Can not find all removed hashes in the siblings",
            )));
        }

        let mut updated_queries: Vec<QueryProof> = vec![];
        for proof_query in proof.queries.iter() {
            if !removed_keys.contains(&proof_query.key()) {
                updated_queries.push(proof_query.clone());
            }
        }
        if proof.queries.len() - updated_queries.len() != removed_keys.len() {
            return Err(SMTError::InvalidInput(String::from(
                "Keys in the queries are not included in all removed keys",
            )));
        }

        Ok(Proof {
            sibling_hashes: updated_sibling_hashes,
            queries: updated_queries,
        })
    }

    pub fn prepare_queries_with_proof_map(
        proof: &Proof,
    ) -> Result<HashMap<Vec<bool>, QueryProofWithProof>, SMTError> {
        let mut queries_with_proof: HashMap<Vec<bool>, QueryProofWithProof> = HashMap::new();
        for query in &proof.queries {
            let binary_bitmap = utils::strip_left_false(&utils::bytes_to_bools(&query.bitmap));
            let key_bools = utils::bytes_to_bools(query.key());
            let binary_path = if binary_bitmap.len() > key_bools.len() {
                return Err(SMTError::InvalidBitmapLen);
            } else {
                key_bools[..binary_bitmap.len()].to_vec()
            };

            queries_with_proof.insert(
                binary_path,
                QueryProofWithProof::new_with_pair(
                    Arc::clone(&query.pair),
                    &binary_bitmap,
                    &[],
                    &[],
                ),
            );
        }

        Ok(queries_with_proof)
    }

    /// calculate_root calculates the merkle root with sibling hashes and multi proofs according to the [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md#proof-verification).
    pub fn calculate_root(
        sibling_hashes: &[Vec<u8>],
        queries: &mut [QueryProofWithProof],
    ) -> Result<Vec<u8>, SMTError> {
        queries.sort_descending();

        let mut sorted_queries = VecDeque::from(queries.to_vec());
        let mut next_sibling_hash = 0;

        while !sorted_queries.is_empty() {
            let query = &sorted_queries.pop_front().unwrap();
            if query.is_zero_height() {
                if next_sibling_hash != sibling_hashes.len() {
                    return Err(SMTError::InvalidInput(String::from(
                        "Not all sibling hashes were used",
                    )));
                }
                return Ok(query.hash.clone());
            }

            let mut sibling_hash: Vec<u8> = vec![];
            if !sorted_queries.is_empty() && query.is_sibling_of(&sorted_queries[0]) {
                let sibling = sorted_queries.pop_front().unwrap();
                // We are merging two branches.
                // Check that the bitmap at the merging point is consistent with the nodes type.
                if Self::is_bitmap_valid(&sibling, query)? {
                    sibling_hash = sibling.hash;
                }
            } else if !query.binary_bitmap[0] {
                sibling_hash = EMPTY_HASH.to_vec();
            } else if query.binary_bitmap[0] {
                if sibling_hashes.len() == next_sibling_hash {
                    return Err(SMTError::InvalidInput(String::from(
                        "no more sibling hashes available",
                    )));
                }
                sibling_hash = sibling_hashes[next_sibling_hash].clone();
                next_sibling_hash += 1;
            }
            insert_and_filter_queries(Self::next_query(query, &sibling_hash), &mut sorted_queries);
        }

        Ok(vec![])
    }

    /// new creates a new SparseMerkleTree.
    pub fn new(root: &[u8], key_length: KeyLength, subtree_height: SubtreeHeight) -> Self {
        let max_number_of_nodes = 1 << subtree_height.u16();
        let r = if root.is_empty() {
            EMPTY_HASH.to_vec()
        } else {
            root.to_vec()
        };
        Self {
            root: Arc::new(Mutex::new(Arc::new(r))),
            key_length,
            subtree_height,
            max_number_of_nodes,
        }
    }

    /// commit updates the db with key-value pairs based on [LIP-0039](https://github.com/LiskHQ/lips/blob/main/proposals/lip-0039.md#root-hash-calculation) with SubTree optimization.
    /// Nodes are batched to "SubTree" for defined height N (4 or 8) to reduce DB call with trade-off of # of hashes.
    /// all the keys for the data must be unique and have the same length.
    pub fn commit(
        &mut self,
        db: &mut impl Actions,
        data: &UpdateData,
    ) -> Result<SharedVec, SMTError> {
        if data.is_empty() {
            return Ok(Arc::clone(&self.root));
        }
        let (update_keys, update_values) = data.entries();
        // check if all keys have the same length
        if !utils::have_all_arrays_same_length(&update_keys, self.key_length.into()) {
            return Err(SMTError::InvalidInput(String::from(
                "all keys must have the same length",
            )));
        }
        // get the root subtree
        let root = self.get_subtree(db, &self.root.lock().unwrap())?;
        // update using the key-value pairs starting from the root (height: 0).
        let new_root = self.update_subtree(db, &update_keys, &update_values, &root, Height(0))?;
        self.root = Arc::new(Mutex::new(new_root.root));
        Ok(Arc::clone(&self.root))
    }

    /// prove returns multi-proof based on the queries.
    /// proof can be inclusion or non-inclusion proof. In case of non-inclusion proof, it will be prove the query key is empty in the tree.
    pub fn prove(
        &mut self,
        db: &mut impl Actions,
        queries: &[Vec<u8>],
    ) -> Result<Proof, SMTError> {
        if queries.is_empty() {
            return Ok(Proof {
                queries: vec![],
                sibling_hashes: vec![],
            });
        }
        let (mut query_with_proofs, ancestor_hashes) = self.generate_sibling_data(db, queries)?;
        let proof_queries = self.get_proof_queries(&query_with_proofs);

        query_with_proofs.sort_descending();

        let mut sibling_hashes = vec![];
        let mut query_with_proofs = VecDeque::from(query_with_proofs);
        calculate_sibling_hashes(
            &mut query_with_proofs,
            &ancestor_hashes,
            &mut sibling_hashes,
        );

        Ok(Proof {
            queries: proof_queries,
            sibling_hashes,
        })
    }

    /// verify checks if the provided proof is valid or not against the provided root.
    /// Note that in case of non-inclusion proof, it will be still be valid.
    pub fn verify(
        query_keys: &[Vec<u8>],
        proof: &Proof,
        root: &[u8],
        key_length: KeyLength,
    ) -> Result<bool, SMTError> {
        let mut filtered_proof =
            match Self::verify_and_prepare_proof_map(proof, query_keys, key_length) {
                Ok(v) => v,
                Err(_) => return Ok(false),
            };

        match SparseMerkleTree::calculate_root(&proof.sibling_hashes, &mut filtered_proof) {
            Ok(computed_root) => Ok(utils::is_bytes_equal(root, &computed_root)),
            Err(_) => Ok(false),
        }
    }

    // remove_keys_from_proof removes keys from proof and returns a new proof without them.
    pub fn remove_keys_from_proof(
        proof: &Proof,
        removed_keys: &[&[u8]],
    ) -> Result<Proof, SMTError> {
        let filter_map = Self::prepare_queries_with_proof_map(proof)?;
        let mut filtered_proof = filter_map
            .values()
            .cloned()
            .collect::<Vec<QueryProofWithProof>>();
        filtered_proof.sort_descending();

        let mut sorted_queries = VecDeque::from(filtered_proof.to_vec());
        let mut added_sibling_hashes: Vec<(usize, Vec<u8>)> = vec![];
        let mut removed_sibling_hashes: NestedVec = vec![];
        let mut next_sibling_hash_index = 0;
        let mut added_sibling_hash_index = 0;

        for query in sorted_queries.iter_mut() {
            if removed_keys.contains(&query.query_proof.key()) {
                query.is_removed = true;
            }
        }

        while !sorted_queries.is_empty() {
            let query = &mut sorted_queries.pop_front().unwrap();
            if query.is_zero_height() {
                // the top of the tree, so return the merkle root.
                return Self::prepare_result_proof(
                    proof,
                    &added_sibling_hashes,
                    &removed_sibling_hashes,
                    removed_keys,
                    next_sibling_hash_index,
                );
            }

            let mut sibling_hash: Vec<u8> = vec![];
            // there are three cases for the sibling hash:
            if !sorted_queries.is_empty() && query.is_sibling_of(&sorted_queries[0]) {
                // #1. sibling hash is next element of sorted_queries.
                let sibling = sorted_queries.pop_front().unwrap();
                // We are merging two branches.
                // Check that the bitmap at the merging point is consistent with the nodes type.
                if Self::is_bitmap_valid(&sibling, query)? {
                    sibling_hash = sibling.hash.clone();
                }
                // if the branch is being removed, we need to add the sibling hash to the list of hashes to be added.
                if !query.is_removed && sibling.is_removed {
                    added_sibling_hashes.push((added_sibling_hash_index, sibling.hash.clone()));
                    added_sibling_hash_index += 1;
                } else if query.is_removed && !sibling.is_removed {
                    added_sibling_hashes.push((added_sibling_hash_index, query.hash.clone()));
                    added_sibling_hash_index += 1;
                    query.is_removed = false;
                }
            } else if !query.binary_bitmap[0] {
                // #2. sibling hash is a default empty node.
                sibling_hash = EMPTY_HASH.to_vec();
            } else if query.binary_bitmap[0] {
                // #3. sibling hash comes from proof.sibling_hashes.
                if proof.sibling_hashes.len() == next_sibling_hash_index {
                    return Err(SMTError::InvalidInput(String::from(
                        "No more sibling hashes available",
                    )));
                }
                sibling_hash = proof.sibling_hashes[next_sibling_hash_index].clone();
                next_sibling_hash_index += 1;
                added_sibling_hash_index += 1;
                if query.is_removed {
                    removed_sibling_hashes.push(sibling_hash.clone());
                }
            }
            insert_and_filter_queries(Self::next_query(query, &sibling_hash), &mut sorted_queries);
        }

        Err(SMTError::InvalidInput(String::from("Empty")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sparse_merkle_tree::smt_db;

    #[test]
    fn test_subtree() {
        let test_data = vec![
            ("05030302020303001f930f4f669738b026406a872c24db29238731868957ae1de0e5a68bb0cf7da633e508533a13da9c33fc64eb78b18bd0646c82d6316697dece0aee5a3a92e45700082e6af17a61852d01dfc18e859c20b0b974472bf6169295c36ce1380c2550e16c16babfe7d3204f61852d100f553276ad154921988de3797622091f0581884b008b647996849b70889d2a382d8fa2f42405c3bca57189de0be52c92bbc03f0cd21194ddd776cf387a81d0117b6288e6a724ec14a58cdde3c196292191da360da800ec66ad4b484153de040869f8833a30a8fcde4fdf8fcbd78d33c2fb2182dd8ffa3b311d3a72a9aec8560c56c68d665ad54c5644d40ea4fc7ed914d4eea5da3c0400e93bd78ce150412056a9076cf58977ff1a697b1932abdd52d7b978fce69186d3a9cb7274eceac6b0807ce4db0763dc596cd00e59177172de6b5dd1593b33a78500c8c4673053da259999cbc9502aef75c3c0b84bce42b1d1a2d437df88d32b737bd36e7a6410939ac431914de947353f06bbbfc31c86609ec291ed9e13b665f86a", "d6b9f2888480a4fa33fc1d0e0daaef702f0ab41dd8875eee80b7c312011e5191", vec![3, 3, 2, 2, 3, 3]),
            ("02010202020049720db77a5ca853713493d4e11926b417af0cae746a305a52f555738eed47cad58c7809f5cf4119cc0f25c224f7124d15b5d62ba93bc3d948db32871026f068018dfe7dfa8fb4a5a268168638c8cce0e26f87a227320aee691f8872ed6a3aba0e", "0989d2ac315f25669f69e9bef067b9c41310964d76565196ac4fb92452c68955", vec![1,2,2]),
            ("0f0404040404040404040404040404040401bbacc7102a28f2eecd0e4de3c130064e653d0118b1dc4129095901f190e70034019dcb747007aca526d4b0782ed20a88a5d48a4ab6276378bada201ab5b6e4d75b01e89b7270dd0ad80207e11422bfc28f8cda8932d59b1082486fa1bf5626ea0aba01858c61150861b89516244e07cfd9d3ebcb12b2d44c2de4e7e2faed96717202eb01f9437e84b231d85f7fc2690ed54b09e85c2e0fc98b26430f10418065374e40bf0189ae2184c9a2e70656ce37c89c903b258198ad6e9db66f135780f66d8613a6fd01058c3bef2957b130622e752f0a81ee8dcf60b4685675eb88e39d5150c954fe220161543e80c5356f580f8e7e4548576486ee754ffe22f4dd122ef48e41bffc7adc01f55a1089a16835a4cbe8b5e12227575ecfd99cd951e34b409f9b2ace6f25a49701e5dfbf3ecaf909728248a751e1a75f3b626777094fe1aab03ae6f526ddac799a01f88ad8cd4aec6cc4f8d2c2bc4a5f368fc9b877685eb55673baa01d652fa4c82b0182f8fb577797274de4f48d8bd7cc5a77068ea3c60477e8552b38c926466eba1101c149d0c79bc1355d763d01690139fd187a84488d534e7e38e4772279c3826b9b01006afab486675b0e3f9b6b06283da947df6749269fb8621afe843d5df942bce7011ead1b569f80edffa2044bf9d8b8703b970ca741b821127d6da69da83b52294f01c1a9d57b050c3ba96aca78a26c5eebc76bb51acab78ce70ed3bdea1ca9143cd8", "2c76277c959e70205fff49ef8732047516cf07e18758345ca56005e732ca2d17", vec![4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]),
        ];

        for (data, hash, structure) in test_data {
            let decoded_data = hex::decode(data).unwrap();
            let tree = SubTree::new(&decoded_data, KeyLength(32)).unwrap();
            let decoded_hash = hex::decode(hash).unwrap();
            assert_eq!(tree.structure, structure);
            assert_eq!(*tree.root, decoded_hash);
        }
    }
    #[test]
    fn test_subtree_encode() {
        let test_data = vec![
            ("05030302020303001f930f4f669738b026406a872c24db29238731868957ae1de0e5a68bb0cf7da633e508533a13da9c33fc64eb78b18bd0646c82d6316697dece0aee5a3a92e45700082e6af17a61852d01dfc18e859c20b0b974472bf6169295c36ce1380c2550e16c16babfe7d3204f61852d100f553276ad154921988de3797622091f0581884b008b647996849b70889d2a382d8fa2f42405c3bca57189de0be52c92bbc03f0cd21194ddd776cf387a81d0117b6288e6a724ec14a58cdde3c196292191da360da800ec66ad4b484153de040869f8833a30a8fcde4fdf8fcbd78d33c2fb2182dd8ffa3b311d3a72a9aec8560c56c68d665ad54c5644d40ea4fc7ed914d4eea5da3c0400e93bd78ce150412056a9076cf58977ff1a697b1932abdd52d7b978fce69186d3a9cb7274eceac6b0807ce4db0763dc596cd00e59177172de6b5dd1593b33a78500c8c4673053da259999cbc9502aef75c3c0b84bce42b1d1a2d437df88d32b737bd36e7a6410939ac431914de947353f06bbbfc31c86609ec291ed9e13b665f86a", "7a208dc2a21cb829e5fa4dc7d876bef8e52ddd23ae5ea24c2567b264bcd91a23", vec![3, 3, 2, 2, 3, 3]),
            ("02010202020049720db77a5ca853713493d4e11926b417af0cae746a305a52f555738eed47cad58c7809f5cf4119cc0f25c224f7124d15b5d62ba93bc3d948db32871026f068018dfe7dfa8fb4a5a268168638c8cce0e26f87a227320aee691f8872ed6a3aba0e", "c0fcf4b2571622905dde0884ef56d494ad3481d28fa167466f970f2c633e2925", vec![1,2,2]),
            ("0f0404040404040404040404040404040401bbacc7102a28f2eecd0e4de3c130064e653d0118b1dc4129095901f190e70034019dcb747007aca526d4b0782ed20a88a5d48a4ab6276378bada201ab5b6e4d75b01e89b7270dd0ad80207e11422bfc28f8cda8932d59b1082486fa1bf5626ea0aba01858c61150861b89516244e07cfd9d3ebcb12b2d44c2de4e7e2faed96717202eb01f9437e84b231d85f7fc2690ed54b09e85c2e0fc98b26430f10418065374e40bf0189ae2184c9a2e70656ce37c89c903b258198ad6e9db66f135780f66d8613a6fd01058c3bef2957b130622e752f0a81ee8dcf60b4685675eb88e39d5150c954fe220161543e80c5356f580f8e7e4548576486ee754ffe22f4dd122ef48e41bffc7adc01f55a1089a16835a4cbe8b5e12227575ecfd99cd951e34b409f9b2ace6f25a49701e5dfbf3ecaf909728248a751e1a75f3b626777094fe1aab03ae6f526ddac799a01f88ad8cd4aec6cc4f8d2c2bc4a5f368fc9b877685eb55673baa01d652fa4c82b0182f8fb577797274de4f48d8bd7cc5a77068ea3c60477e8552b38c926466eba1101c149d0c79bc1355d763d01690139fd187a84488d534e7e38e4772279c3826b9b01006afab486675b0e3f9b6b06283da947df6749269fb8621afe843d5df942bce7011ead1b569f80edffa2044bf9d8b8703b970ca741b821127d6da69da83b52294f01c1a9d57b050c3ba96aca78a26c5eebc76bb51acab78ce70ed3bdea1ca9143cd8", "5a2f1f740cbea0944d5182fe8ef9190d7a07e8601d0b9fc1137d48b94ce73407", vec![4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]),
        ];

        for (data, _, _) in test_data {
            let decoded_data = hex::decode(data).unwrap();
            let tree = SubTree::new(&decoded_data, KeyLength(32)).unwrap();
            assert_eq!(tree.encode(), decoded_data);
        }
    }

    #[test]
    fn test_empty_tree() {
        let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
        let data = UpdateData { data: Cache::new() };
        let mut db = smt_db::InMemorySmtDB::default();
        let result = tree.commit(&mut db, &data);

        assert_eq!(
            **result.unwrap().lock().unwrap(),
            hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap()
        );
    }

    #[test]
    fn test_key_length_invalid_size() {
        let test_data = vec![
            (
                vec![
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa0", // invalid key size
                ],
                vec![
                    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                ],
            ),
            (
                vec![
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa07b3c", // invalid key size
                ],
                vec![
                    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                ],
            ),
        ];

        for (keys, values) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data);

            assert_eq!(
                result.err(),
                Some(SMTError::InvalidInput(String::from(
                    "all keys must have the same length"
                )))
            );
        }
    }

    #[test]
    fn test_small_tree_0() {
        let test_data = vec![(
            vec!["6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"],
            vec!["1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a"],
            "5fa3f96b5a13d96f18db867a16addf7483ab3448b3a267f774e1479b8dd1193c",
        )];

        for (keys, values, root) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data);

            assert_eq!(
                **result.unwrap().lock().unwrap(),
                hex::decode(root).unwrap()
            );
        }
    }

    #[test]
    fn test_small_tree_1() {
        let test_data = vec![(
            vec![
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
            ],
            vec![
                "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
            ],
            "5b693f1384c3e07b2a5f91a616d9f3676b5724b9664849b641d6139b5ad11b1a",
        )];

        for (keys, values, root) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data);

            assert_eq!(
                **result.unwrap().lock().unwrap(),
                hex::decode(root).unwrap()
            );
        }
    }

    #[test]
    fn test_small_tree_2() {
        let test_data = vec![(
            vec![
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
            ],
            vec![
                "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
            ],
            "c7c343f08404db36d05d6c76c886a371e8d2ab1fe82f9dc14a985e8e204d1d20",
        )];

        for (keys, values, root) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data);

            assert_eq!(
                **result.unwrap().lock().unwrap(),
                hex::decode(root).unwrap()
            );
        }
    }

    #[test]
    fn test_small_tree_3() {
        let test_data = vec![(
            vec![
                "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
            ],
            vec![
                "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
            ],
            "248c25930001a47d344e6807f25edad5195d71ad9c96b73a99cccc7906343112",
        )];

        for (keys, values, root) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data);

            assert_eq!(
                **result.unwrap().lock().unwrap(),
                hex::decode(root).unwrap()
            );
        }
    }

    #[test]
    fn test_proof_verify_key_length() {
        let test_data = vec![(
            vec!["ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879"],
            vec!["b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215"],
            "641de8cb1043ed944f81e9fdb2a185437e9df861f87eb8176b9431d0340b2b21",
            vec!["ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879"],
        )];

        for (keys, values, root, query_keys) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }

            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data).unwrap();
            assert_eq!(**result.lock().unwrap(), hex::decode(root).unwrap());

            // The query key length is 29, which is not equal to the key length of the tree.
            let proof: Proof = Proof {
                sibling_hashes: vec![],
                queries: vec![QueryProof {
                    pair: Arc::new(KVPair(
                        hex::decode(query_keys[0]).unwrap()[0..29].to_vec(),
                        hex::decode(values[0]).unwrap(),
                    )),
                    bitmap: Arc::new(vec![]),
                }],
            };

            // The proof verification should fail.
            assert!(!SparseMerkleTree::verify(
                &query_keys
                    .iter()
                    .map(|k| hex::decode(k).unwrap())
                    .collect::<NestedVec>(),
                &proof,
                &result.lock().unwrap(),
                KeyLength(32)
            )
            .unwrap());
        }
    }

    #[test]
    fn test_remove_keys_from_proof() {
        let test_data = vec![
            (
                vec![
                    "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
                    "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
                    "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
                    "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
                    "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                    "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
                    "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                    "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
                    "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
                    "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
                    "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                    "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
                    "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
                    "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
                    "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
                    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                    "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
                    "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                    "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
                    "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
                ],
                vec![
                    "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
                    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                    "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
                    "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
                    "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
                    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                    "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
                    "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
                    "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                    "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
                    "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                    "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
                    "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
                    "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
                    "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                    "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
                    "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                    "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                    "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                    "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
                    "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
                    "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
                    "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
                    "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                    "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
                    "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                    "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
                    "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
                    "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
                ],
                "2770cbf448782d4a24a53135741e1c7653d065c53e2cf150d431aad77993e2a0",
                vec![
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                ],
                vec![
                    "7aada8d512d761dcd18031059d1c17d80d9c79f67ae3babf5cabf260d5790cae",
                    "7fd4719a219a224ecd4f1ef19eac8ccbc244a7368857707deb3b65c351c0c0a7",
                    "93b98c20a3b72d7aee973c2374d95db8a79912be7a4641dfc28f26d441bf7890",
                    "44195ef71f7a750609cd0edcfbce505092216d06b4bffa52607eb2770d72a9a7",
                    "e8527ba9c82736c6a644049a88e4a0c7eabbb23adf20d0dd40b7f8d474642a27",
                    "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                    "32bb08752a5dd3a88d168dc52b3e7c53a89a6679b23ceb2c5c26a7af495bd128",
                    "0498d41cc43f5ee1be71781513164d2dfea38bf0ee3289f02d18127f7c4406d6",
                    "f03feb21e428c640620dc2698787943c5d67b895ff316594bdd36493d2deca2b",
                    "03662db407793a42f74699970ea28cce39304449eb900d15003a2b79bb9d1e7b",
                    "f4fba601b5a1624731f3c4153defe761f977d167ea5a77a110dd571cbcd9bc2a",
                    "884d43dbcd182a451954f4633481126ab414e9650a3743203922e528228883a4",
                ],
            ),
            (
                vec![
                    "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
                    "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
                    "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
                    "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
                    "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                    "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
                    "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                    "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
                    "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
                    "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
                    "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                    "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
                    "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
                    "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
                    "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
                    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                    "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
                    "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                    "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
                    "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
                ],
                vec![
                    "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
                    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                    "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
                    "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
                    "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
                    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                    "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
                    "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
                    "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                    "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
                    "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                    "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
                    "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
                    "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
                    "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                    "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
                    "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                    "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                    "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                    "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
                    "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
                    "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
                    "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
                    "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                    "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
                    "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                    "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
                    "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
                    "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
                ],
                "2770cbf448782d4a24a53135741e1c7653d065c53e2cf150d431aad77993e2a0",
                vec![
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                ],
                vec![
                    "7aada8d512d761dcd18031059d1c17d80d9c79f67ae3babf5cabf260d5790cae",
                    "7fd4719a219a224ecd4f1ef19eac8ccbc244a7368857707deb3b65c351c0c0a7",
                    "93b98c20a3b72d7aee973c2374d95db8a79912be7a4641dfc28f26d441bf7890",
                    "44195ef71f7a750609cd0edcfbce505092216d06b4bffa52607eb2770d72a9a7",
                    "e8527ba9c82736c6a644049a88e4a0c7eabbb23adf20d0dd40b7f8d474642a27",
                    "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                    "32bb08752a5dd3a88d168dc52b3e7c53a89a6679b23ceb2c5c26a7af495bd128",
                    "0498d41cc43f5ee1be71781513164d2dfea38bf0ee3289f02d18127f7c4406d6",
                    "f03feb21e428c640620dc2698787943c5d67b895ff316594bdd36493d2deca2b",
                    "03662db407793a42f74699970ea28cce39304449eb900d15003a2b79bb9d1e7b",
                    "f4fba601b5a1624731f3c4153defe761f977d167ea5a77a110dd571cbcd9bc2a",
                    "884d43dbcd182a451954f4633481126ab414e9650a3743203922e528228883a4",
                ],
            ),
            (
                vec![
                    "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
                    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                    "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
                    "265fda17a34611b1533d8a281ff680dc5791b0ce0a11c25b35e11c8e75685509",
                    "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
                    "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
                    "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                    "9652595f37edd08c51dfa26567e6cd76e6fa2709c3e578478ca398d316837a7a",
                    "cdb4ee2aea69cc6a83331bbe96dc2caa9a299d21329efb0336fc02a82e1839a8",
                    "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
                    "3973e022e93220f9212c18d0d0c543ae7c309e46640da93a4a0314de999f5112",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "8a5edab282632443219e051e4ade2d1d5bbc671c781051bf1437897cbdfea0f1",
                    "684888c0ebb17f374298b65ee2807526c066094c701bcc7ebbe1c1095f494fc1",
                    "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
                    "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
                    "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
                    "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
                    "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
                    "951dcee3a7a4f3aac67ec76a2ce4469cc76df650f134bf2572bf60a65c982338",
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068",
                    "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
                    "32ebb1abcc1c601ceb9c4e3c4faba0caa5b85bb98c4f1e6612c40faa528a91c9",
                    "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                    "bb7208bc9b5d7c04f1236a82a0093a5e33f40423d5ba8d4266f7092c3ba43b62",
                    "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
                    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                    "334359b90efed75da5f0ada1d5e6b256f4a6bd0aee7eb39c0f90182a021ffc8b",
                    "09fc96082d34c2dfc1295d92073b5ea1dc8ef8da95f14dfded011ffb96d3e54b",
                    "8a331fdde7032f33a71e1b2e257d80166e348e00fcb17914f48bdb57a1c63007",
                    "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                    "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
                    "ba5ec51d07a4ac0e951608704431d59a02b21a4e951acc10505a8dc407c501ee",
                    "ffe679bb831c95b67dc17819c63c5090d221aac6f4c7bf530f594ab43d21fa1e",
                    "d03502c43d74a30b936740a9517dc4ea2b2ad7168caa0a774cefe793ce0b33e7",
                    "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                    "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
                    "a318c24216defe206feeb73ef5be00033fa9c4a74d0b967f6532a26ca5906d3b",
                    "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                    "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
                    "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
                    "1f18d650d205d71d934c3646ff5fac1c096ba52eba4cf758b865364f4167d3cd",
                    "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    "bbf3f11cb5b43e700273a78d12de55e4a7eab741ed2abf13787a4d2dc832b8ec",
                    "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
                ],
                vec![
                    "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
                    "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                    "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
                    "e368b5f8bac32462da14cda3a2c944365cbf7f34a5db8aa4e9d85abc21cf8f8a",
                    "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
                    "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
                    "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                    "2651c51550722c13909ec43f50a1da637f907f7a307e8f60695ae23d3380abad",
                    "8a1fe157beac6df9db1f519afed60928eeb623c104a53a62af6b88c423d47e35",
                    "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
                    "fc62b10ec59efa8041f5a6c924d7c91572c1bbda280d9e01312b660804df1d47",
                    "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                    "e344fcf046503fd53bb404197dac9c86405f8bd53b751e40bfa0e386df112f0f",
                    "ff122c0ea37f12c5c0f330b2616791df8cb8cc8f1114304afbf0cff5d79cec54",
                    "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
                    "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
                    "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
                    "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
                    "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
                    "bf1d535d30ebf4b7e639721faa475ea6e5a884f6468929101347e665b90fccdd",
                    "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                    "24944f33566d9ed9c410ae72f89454ac6f0cfee446590c01751f094e185e8978",
                    "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
                    "d8909983be3179a28734edac2ad9e1d0364c8e15e6e8cdc1363d9969d23c7d95",
                    "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                    "7e0e5207f9102c79bd355ccafc329b517e0d6c2b509b37f30cfc39538992cb36",
                    "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
                    "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                    "7c8b1ed7e1074189bf7ff3618e97b4ce88f25c289f827c30fabfbe6607058af6",
                    "d4880b6be079f51ee991b52f2e92636197de9c8a4063f69987eff619bb934872",
                    "ef79a95edac9b7119192b7765ef48dc8c7ecc0db12b28d9f39b5b4dedcc98ccd",
                    "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                    "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                    "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
                    "2ec9b3cc687e93871f755d6c9962f62f351598ba779d9838aa2b68c8ef309f50",
                    "084aa2e4bc2defcd2c409c1916023acd6972624cf88112280b6dacde48367b0c",
                    "0ca5765ffb7eb99901483c2cda1dd0209cef517e96e962b8c92c1668e5334d43",
                    "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
                    "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
                    "0272614c70432bc1f94b8739603ba170ad5f4866a9936f37c463767bda7d005d",
                    "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                    "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
                    "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
                    "a2215262d04d393d4e050d216733138a4e28c0b46375e84b7a34a218db8dc856",
                    "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
                    "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                    "247f88a674f9f504e95f846272b120deaa29b0ae6b0b9069689488ba3c8e90ab",
                    "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
                ],
                "401dba08d0bc4edf4d1f816742b59385f6617e156904d6bee7e9b4f096abf847",
                vec![
                    "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                    "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                    "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                    "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                    "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                ],
                vec![
                    "ccc998cd27d1c7c74de713dc9e31fed4c46ecb7375c9fc681e157d1bcb413bf3",
                    "8155096662f2bbde82e17684a0a0d1c4afcd316f3b45731e07ccd6cd69d1ee4f",
                    "7aada8d512d761dcd18031059d1c17d80d9c79f67ae3babf5cabf260d5790cae",
                    "7fd4719a219a224ecd4f1ef19eac8ccbc244a7368857707deb3b65c351c0c0a7",
                    "9b5c0979c8f3b7d7651edc5532435014b5beaa980cfd9cab3fdcae36d9df4b00",
                    "44195ef71f7a750609cd0edcfbce505092216d06b4bffa52607eb2770d72a9a7",
                    "74b28dfde1df60fa62e78609e0b7c457d91bbe732ff82737d4297085ba12c866",
                    "e8527ba9c82736c6a644049a88e4a0c7eabbb23adf20d0dd40b7f8d474642a27",
                    "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                    "2303f37ce9621f2ce5f3c046c95cdf221fdd53e01d97aaa339c7c06521cf007f",
                    "47da630f0a89f184eb1d296a3f17cea1a750e1c7c1a3b1ef2f4f60034d64c9fe",
                    "ff69738cfb9678d0677b35780f8c5d18e2e59725632c642fa5ac4f7baf6e9f32",
                    "32bb08752a5dd3a88d168dc52b3e7c53a89a6679b23ceb2c5c26a7af495bd128",
                    "0498d41cc43f5ee1be71781513164d2dfea38bf0ee3289f02d18127f7c4406d6",
                    "b33bb9602edceb6854bd539ee77bec8bea656ce86b62be30bdee43876d3ebee2",
                    "caefa0f9757b2c6820e34c43a48133d86207ac53abcae2be34f4bb34a31bc855",
                    "f856f0a29502b912714605395a6394373877e5ffdc544bc5477054866b88f16e",
                    "9636e2b9312e0ae556db5a7c926a82e7597764d40059bd796c3ae215e8ed1e3d",
                ],
            ),
            (
                vec![
                    "21210b61f97c78fefd4c3a266e28f9ce1f43e0a37b0ed8aa945b352c3bc55cd4",
                    "0a57caaa7d01f5dce091a38a782324b63e4d3ebb2346f7f16e5e23263f7efbac",
                    "ea5b73706e2dfbbf5797ce67b8647f9e31da9506f61a81a687300139477b20d7",
                    "29490422e4f092c19700b22869a470f0d450cc6530cc4ab523395ee280879adb",
                    "a8743e322199cca0df557135ac0b8f57e5a08ca1abedf60f2213ae4f78a46c75",
                    "dc734fc41ec0ba1d06f38c6d0992e5e8f78cb62020883c9c70de1838534b4bc8",
                    "6e34df79e2ede828b4d2aa0a9aebe4367fb17dd559f1c3fe83742b2c4f92c58b",
                    "e5c235583dffca60f513d6d22a4d194fdf793dd437bfd711ead22cc3d820b13e",
                    "7b6513591c542363414b45daf4d1d3712f13a5d6f0235f6b85ed6abeeeffa693",
                    "a3266edb545befeb0b8ac279b5ba3ad89d972b2bdda291b2997f17b6c5661998",
                    "961d1d6e48e8bce1a1a043b35c1d2905f56b729f9517131039ab49065ce42f08",
                    "6d89090a59fc0a1095cdb39718d91c21a387dbe3f1313d203cab9f3fbb6ea71a",
                    "0a43663899d5e3697fbe30e0ed03bc94077651bf267316034791f9d74a880efd",
                    "f1e467327b7434c610e552337a5b133dbafa4187c90a904ffce143cb3c3ef1c5",
                    "5c5a63ea59240410d291571f26b7b082c645e9935b572cacbb01f2598d9e650a",
                    "9c8e90a2b87e6b0de76ebd8a7473804466230a8bb1a1d010dc95ee12f528724b",
                    "2ae39e138cc695cceef4fd17bf3f1ef399064e2a45d70578a0ce21442030d5aa",
                    "6c335694ff8ed2a665f30afa954d2cca11d779fa31df32988eca82f4b863825e",
                    "83013cf7a3674486b8dbbf4137e40c5f190baa5c1f6ae434a1489403eef09dfb",
                    "7ecf90725cc58e8fd203414a821fa7b2cf81302b4a97882cfe7927507eba7ff4",
                    "c16999fded3e396869e55dbf7744e9df1500bdb97e79eedc4b7d97f4448e20b6",
                    "6690f3bf1cd17516d4336de9e8a5dd3a036b559c3940230d5c779be4bd84d29e",
                    "1ce7b6d7e06e988092ddb4d9e08ba11bf329c21d873f9a6cf3436482d657d88d",
                    "77e0c81547a192fdf201027d106a25e3b67974f5818ff894498ce53c62a273ee",
                    "b84834ea9b3f77346387b946e2b5579e75e49b5ddf1358acb8a83ad0d2b4c8a0",
                    "52f38b71f5a9409ada100f7cb7b8736ec2abc04e9a43e1903672f61dcbbf18a6",
                    "c5e9c83494ecb01d60c67686a6fec40cd3953845f67611004beb6b01abc74c7b",
                    "166fa383d238a5a004543c6b42c913b288d9ce314b4c984340ebfbb754415251",
                    "b1f3c716d22c2c9add1f2ae93b9619aea36bdf1d375f822bc266f88969e29d4f",
                    "ebab7db679ee6b4552397bbc308b738a65504bb1b853bec18b7991ebdf7f2783",
                    "b455086fcf84892566d9521b6fa539eaf29df9227e238af4941b9072fee68d8e",
                    "eb6f32c239fbec9ecd78209d4006048b9d170d0c1f2fb6192097d783ae1380ab",
                    "a423e7cb2384e091467276b310536a83d8bb325a71597f9900d73a52c82c7137",
                    "c1e158bdf0d3fb3ccf1851390607a6a8a374f969ee4bddfada59d555f68b9a03",
                    "62c92c73a88528825f056071b487a6a270c8a4a387ce12724b2a5c984fe4e39e",
                    "d063520eac73b4eeddb354676fe919a089bc1573670a0929cf9befc5ee8f357f",
                    "71b16968c679e9d25b37d4de174b3016b8588c5b6e06c9100f107144194be39a",
                    "f4eec52aac2504b97a2a074fe7a30d35f8ea4a93eef335b665066d9badfc950a",
                    "f57b0ad0f1a34bb4e86accbec02a510ff3eeb01227ff6a9c470988c76ec6c3ee",
                    "571513a9b592ab83704f8b21cd3f1134cdb6cb3496aed3e677e61abf1971cf0d",
                    "e66becdfe96c1bca997eff67915bc8f0902d481221a60fb65d90a1048d3b7e97",
                    "3285f3fbf0f80447f3da1145e468d4f570313e50a94a83565099b777624111bd",
                    "45ba72128026896228a919e34e263ce336475b6d2724fdaa632c699b20bf0cea",
                    "124efe57d873d27930b5ca20dc0606ac106c9415d8f5fe0501b93327edee27fd",
                    "0cc0fddd53fdc7170d1120de7f503d4d65abff979ced982293ef50f2ccd6e226",
                    "1468b7e16757ea8a35f7ef0634c6b86ba62520036d336be6a76fefa7dc5576a6",
                    "8aa45598c7bd2468c2926528ee7c622f0356702bf7ba65b7e5b7f6c5be45422a",
                    "5fec2e461402697c00ac20f73a7397b434dc4535d2c0cc159f84a06899d32414",
                    "4e1b9f11013aa6c54bd08fdc738b69f1cb5371e9774d9997517af0dc287fc52d",
                    "da747de7a5aa7151e76a6e5cdc72418476aefd3f5bd7ed2ec34871e91056be54",
                    "bb7d4c494c96f548e006f5339f9d558cb3cb0fe5541fa21032a196fe1426bb80",
                    "2caf522da31dc339e54c7de7089682280ab8d1646bf46b7c48591d948878fb5b",
                    "50fe1fd6dc177ada292e26a5a14037dc3cdeb7deaaf41983fa754f1b4bd92d21",
                    "14753f55200fd793843e4ad744c1254c4c2ae96882389f2244a8b2870953cce2",
                    "785acf55f5ce0b2b815c21bc4f722cc9e23e878629db83a7a8f8bc8e117d32e1",
                    "481863d4f2876ea972655649a94f941f3c0dfc3db504ef8dc558c8d396c2e321",
                    "301597d8ae03f46e36cc763dcca79add8dcb9d56f02215c84873a2d2a41db561",
                    "2760c56cd58f0cb596b636e10cb96de60cfeda878cbde6567df5ad7eef83eee1",
                    "9d615bad9eca3e22ccd4e8947395d56acb41966950a3944f4cfbb467a0b1c19b",
                    "b6516aeb07e34f3a929270fcab0765aa7f093d43bbe0b2493603c6687447212a",
                    "990ba63ca2f991a5ccf24c8c1cc3b7ab596fb39c350c3bddc2808d826206cdbe",
                    "7a849a6b8db34092cb02b077986023ed23bf8ce53e3796fc9f95207a6cbef43d",
                    "872d3d8070403e1b9503ca4f88b3659f2fb51ee10aa8f33143b5ded8fcc2b9fd",
                    "0130cea6b391aa2f84f9a8e1377882923deb97610b2aa9f7502fdf985be57c39",
                    "1bad5b320590c0b8b54d79188ea32cf918182858a355284c8260ff0400a54515",
                    "0cf38e5f4bae2e30f8f76e57407529c53ea383fa21c25436b91080df5f3f162a",
                    "b46c3de0490e0cba6f7aa3b0accddf77ee02b942e22f2dc13b4167e26ebd07bc",
                    "7c14ac8b656212a6c31e83ebbaf9db1cad3475fb8503ce6230d513663fdc2953",
                    "d8b7955739b03d2c13144814362ac1cf3263aea89b99ffd4d3c50aa2f3015931",
                    "d51f73ae1499f2a67c8edcb6506587ade45f2919b91a6c2f7e510daee7e26cc2",
                    "5080242949956402863be82721e0c456835004df5059f71c4016f13f996dd498",
                    "5eeaacb199069a7efae65e6e2660e6e4c40f034f3507c6307fa419bda2f89b68",
                    "43ad4e6bd96fe7af330bf9b84a5156b7343b0e00957ad1795c6cfdb705120486",
                    "11f81d81c489ac1de76893e1cae82a66672d4b78ddc62c27af824181a832d6ad",
                    "da25a80d9095064c7b8168cd12c98ee3e046cc7824e7a020cc2d710f236de2e9",
                    "9f1cfdfd776e6f880431570932147e83c854c4f9af3f3670ab36f2f85405d9fd",
                    "6a5127d4d0f2ee5e6b87cd5a39e1ee215bf8d17009bcec96d88828a5321e1be4",
                    "d0e73e5cb3150e84ac3520f5a9bccd225f4a2214801ae504c060a977c3f25add",
                    "48bb3d896ac98068d4976fe73c9723b870d342e091c56826169b7aa491a619de",
                    "25ddb8af44e3222276a491ff1cbd6781c324bd59c3ccfc3f3168c96ffbed83ce",
                    "49bb88f9c75737f63fee583044ec1d17f3a108872f368d65bdb5be0e43e01076",
                    "a4d8d370e108977a01739ce8ea8957f585005d75fd50d4f30508db3c59156db8",
                    "984b8b67f8cec9967ce207a783e76d6731b4bc8d42b9f853be16ebf7827a6dd2",
                    "7bd2b56d98722c417ff3a36db1eece25d1f7f16498a920775c570aaec11042bb",
                    "2a6e6aeb2fb6fd95a81591fa61f6e7e5a70af1273d5324e03bb0d6d77b332f4c",
                    "321ee2dfdad92ac2229607994f5a9e1688e99116b3b210c67228a644ea894681",
                    "df5d8ed7b94f3fd9851f0a0ea8a4626db32c0508a57d29a2527fcc3773dbf94d",
                    "778d23b624c2a606cd4156d4b9e87f4b963a863197e720f1aabb73a4d06e6845",
                    "9cd4d919e1036f8e4f89c0531c1d5220f9015a61b3377bbdfb0470dc5ed0480f",
                    "2f16168dcd74011db16616b4f18f4d0da6dd69bfb31bb345612c8b53a252e128",
                    "d6a5a8bc3c2e628a481683dcb4f718bc6d7626d9d2e7b6d1e4b5450c572e6d9f",
                    "062adc058e9af3fef721c8373e9f6047181b9f984e660554ac66d3afa149bb77",
                    "1f66d008d5feee096868b568ffd916c3ec6644d155c0bb69fb169af674f18ebe",
                    "0e98ef871f756a4bc8afa1106e13ee8c2e8c311dbdf0364ba37252e73f97c719",
                    "fa368b8c5d4d85883c965ca5c0ba8ad794df670a7fb56ec7de711f3618989430",
                    "ba3a5cd72eb6513d7ab8d5b0c72727fcb385692ea26ee6cb0a63f227e8ebd734",
                    "869c97475ba635a3d657df86b60aee0907281a780947da62e917c2f54f1e5182",
                    "d2faf70f2938836d9355c195a7e55899d1cd602950721087bd94002dca6b1994",
                    "3f9cc2ccbc7fa149620c9c8ed3e02b8fb41a878f236acdba3f5899694df63a4d",
                    "23eef2af3b7d9869545629a6f93b621d4c6790f5abae48bd70b80b7332b39fb5",
                ],
                vec![
                    "40e13f881026901e8cff7d596204d806d2ed502cb8d81a4586b0d442f44921a3",
                    "2ebd50879e98349a20001c17efe31b007593c80716bcad83f6ee80447cf18574",
                    "791dc2addc9d5634fd1690f16a040eddd1793d6b370e91118472e1c43372de3f",
                    "2655cdc84676e20572b3d2814eb2e0364abd9c6ee0130594281cbb79f07b4a9d",
                    "b7176881b2781ddb625881306b1ffb4c639413e94b46e1f98a7d79e4d17ddab1",
                    "694df7380d22286a366d7de429976938a399e9463244366445c36ad045e6e955",
                    "e6ed308385b595f4e579bcbede4fd7fd57d574819e1abf1c1b7b2773009a15f0",
                    "a2b598556cbd97ea392ab5e9f2740f5ddbe77175915c62e9dae2da366a39417d",
                    "341064dcc5278aeb141ba742e89359de820e966b35f129d2958d92805a970a6b",
                    "0d84e02bd5141a6fda84b68094af97076a97262474b53085131b8f0b99e239c1",
                    "bdc8ea341a0d75a38dd0c281312be62e8767405363f6dcc3511dc056dd0c2311",
                    "e0d74038b3446cac61b4e737129b3dd3321916282f392e185e5aaf84ee005d2b",
                    "4f6efe6f3ec6ea536f06082795ef06fc0ca0dc9702ce2131a7a605c5b274055f",
                    "2f3c6ea8723920c647cac4cee5f1ad1e1396583eb887032c7d513c3f27a9d681",
                    "50b3720b3bafdcccee1096d6d602119abeca05c25503f8c92d74a760f8e0bca2",
                    "f8e4990303eaa241a17dd7da695f18c2f409ed66a07b510dfcca499a45e7d0ec",
                    "9def97d1159e813336bed3c551edd5c9de8d9255dd69f5e8c97bea75ce7cba44",
                    "2eb58b377bbc5bdc59715cda3013a609978b808834c8b99bb5434a7141788845",
                    "38f0357996c53c2d38cb988ef315b6f264855f150a45bb484ad3868164713483",
                    "87a537149f1224129af2ba22e045e9f350f450e139d743b3ae6bb8658155751a",
                    "ba060ebfe9f76cd0d0183d1ff2a505b8d46fc7894b4c816fa811a22f20e34b92",
                    "99ca93e3d65071edd136f8a45437d6a10867d00dd54187dd2e4f7e1fcb1b1437",
                    "36a268e980643da7a44a9bced8d5b16f3e23fcc275deb6e314a637ba0e789112",
                    "63af837276a8ab8efd2515212fddc928795eabbeb9e3f412599d26a9ec8fbee2",
                    "2540eba0b00b656f4237a9119d7abdded69ac4c58c7cad412ab8f1577b889038",
                    "435376bcaa19b8489e8023d1284eeeae6d3fef666671b82b90b952b57fca5c3a",
                    "c3cdc5f3aa929b6d33004f7a6a483c21480d023808a962e9cdf474318e078ba5",
                    "e0955ff672903114f0f10a4c0d6ee9494d8f86e3840641e7aa7f74d9bf9d8319",
                    "e3da2c1650a362c73479ca3bbbc0590197b40045bf3534eb95bb1d79ff04fb03",
                    "1fdeb99d857681e17fb04d8134765f46ba826e885645bc98f892892d6c3d9cdb",
                    "413bcac16fe3d9c65764b32a84184fb49f26b13dc6c7de95eb4ede1f7c8a20d6",
                    "f51367d9f372a913ab80c5535e21106708e865b62f7dc9503344a20882dc555a",
                    "182b98e61dfa6cd9b1ed29eb84151f40b6cb04dd632a594222b6f10e17b353fd",
                    "be24e76d7e3c6d9d15ec52705e26e930f12aa6661b82c78b5ea07764c2b0a52d",
                    "2b3aa7cdd535274ad11146bc9c7ed320fe976507756250269deb150fd8070889",
                    "a336155e5c31ab2f5f83e4a9bdd2d5a6c8e159e5916ae3462b829f57859fb41e",
                    "017d07b579bab2b6618325361c44751c4f840db1b9d8ef8a6aabf8f64f9d5df3",
                    "7135af678793cf50eabb194599ff96d770949ad0655f2a8e9d23fc354b5f465b",
                    "ed14ddcc17c4f3510a2672c5ed587c66f3b5bacc642b4f98b5dff197ae10de77",
                    "72f0e01bb29bbb72e02a8e609003c2ce2b6e4182fa906c05cf5b801dfa61bf62",
                    "35454a0eb39aeff9d4267b1cdf74a5ef3c56d37e1c4f0c66b841bdaae1305359",
                    "10e732dd3592c26fdb9abbb96b2718c6587e550402e07ed7d6331caabcb26766",
                    "d73af8ecf2680dfe6a26cf2c5d0c044562f1a402e491bf8dc25d88e26d803494",
                    "0e2d7af103aeb1b5f605152eaed2cfa2b106882415652d535a6fe8b63112478d",
                    "6534d5bf0bb0da713a3d47617d51264aaa9eb0e91b5398451ad0a6e273576462",
                    "a872893a42fc61b0e4c60b8a17eb9e4311a86dd45d598e55a654539df361c0f6",
                    "72cc6ecb1160e2f9880c260ea09f7380dd8b085a327421cc82c2f7b9ae5a8697",
                    "2535da7b9656307e542c36119255f9fde2a9d0df901dccb0a15dcd612957fec3",
                    "3fa988463a817c011a5c2473adbf417db5379c8c9d6785718e61a08060b935f5",
                    "44fed838badef937705c89f759501bdc0d7b4a0b811573dde4a28cc6a2ffd285",
                    "646252785992cca2df7e0f43b000b7330729e8b2ac475164cf637a17cd53904b",
                    "b7d9d401214903c354d79d67de6710e73fbbb914f6dbde376713306e327e489f",
                    "14fdbafce811d2444ea02c9b4d7dc4368ca6096a40bf1b410361c4625901d123",
                    "5eb43b960205e8ddacc043c44878f40a8bcf1aa57f2207f88ece3ad8f4ebf3b8",
                    "071549fcfdfabf1a712b8239a46ce15025c308c2edfeb768749ca72ecd75aaa0",
                    "544fb30b55cfd67bdafa0d04d68b67ecfabb44cec257cc67cde10bdf2a9f2bb2",
                    "49deddbf97b2abe8136a1a5281d2ea590b207df3f585f3543adcde1b87a8e64e",
                    "41fe23c39f288081f43b3e1af8a224138b5a9956b4527ad3140276a203a8ecb1",
                    "d1190946ac63e646a3f6e6f256721e362829e73a87c877d11bd6fbac4218ac2d",
                    "bd30bad7260b389e81f345a053743d1594a4816da150576508bb01254d44b3c3",
                    "5a68bff628a2015e70b545f57cc3972eb03f5567d21b85d89e478dfff07600bc",
                    "aff892095b5ad1694e10e25541475c88e4325c87ec649c625120d9dd6af0a4f6",
                    "400a9fa42015e54b0996b406c7440e77f4cede0993300880b2f7960ec89b5e0d",
                    "1db7e4f042c5e2b229c36be94eecc89c53a037515f551e1633dc51fb33348681",
                    "71785729d82149959abb93902691bd0d1cbaf3bfa21faeb5dac5e540d9d809a3",
                    "d8dd39e25d6e68cdaee5f649e69d1c5236b963d7dd2e16e870d0c72acef5b5b0",
                    "3bf77faa15ab6de3351db3357b0d6afd066873cce605012ff8a3166ecea92ccc",
                    "604ac5e1b0432637620bc528632f1e70443dadab09a55ca437b75ec33590e106",
                    "48a3900a93b95a300da71e890c4e1a1e837255cbbfccb8f2590e129419870aaf",
                    "4d47cb4a1c2e4050fa5194dbcf02d3d210ab142de6474ea9c1a507e8c73bb2ea",
                    "c179eabada8608eb38dfb381752c8643f74540145ab2809882029d5aa04e4a61",
                    "7cc7756fbc0681f763f1d48a325832952a0d6b7f6bf1d7683b993c85ff8aa9ef",
                    "40eb36bd578391b1632c44d3d3fd03ca5ef951d9783d76c527462ab2913623d2",
                    "45f0c3c622569956e92e124a9abd7042d80e96d12d1830f0917c08f5dbb12fc6",
                    "511703f6d397079529911049daf4248d58cc0b2658e49fc0ea4d061290dbf372",
                    "f4a1409368d2178bc8f0eb151b6430705abc5245e786db831885ebc356431c2a",
                    "00a109b723fe6d0e3d3fa886daa22566f5365e018a297b4f17d0a77ccb7eb533",
                    "3441b72092a2c01b7d043e6e67f4a746222bb6dd7c06f687e2677d8acaa8c9f5",
                    "79f72f7349983b66879482983512aa0c77f073c06c272b1f4a01753053035b15",
                    "f993ff500beab3a9022d66222a3ede5273916f0d8fecfd9633d68c97bde00335",
                    "3d552f56bef8fd9843f1b64997d589013ba3d2626e2623d73d5b6294847ac329",
                    "ff19790dfccc750c64d703b03ae5913b214c9b4e0d093f41000068d24f7f2953",
                    "cce1e55bb9875fc01c429ca4399bb621abbf3e960a2665fa791b10e00a6bf234",
                    "b65d815926f8b3a3bacd419a32a571170670c7f06c43b41d186534310bf57554",
                    "f07c97255fbc21b4339daa373088f76524978ccfd02beac12f517ffcd7cece56",
                    "fb1767be8b13ec5d78e556c49cb74e9a95e88538d1e4474d8c914bd154ef25a1",
                    "42b7decef506ea5685ccbb6d5ec9a8b733db8c4ccb9899285cbf7afcf229d260",
                    "42e1f2582e0429d230e8955953645f47427d9c41663096fcde7b9e7d800a568a",
                    "2939d90fefc426730263e1d72751bff39482557974455aa3dd6fea938bfd2b91",
                    "1042da59527ef0a75eacf98f193b780b08ad6a898832f98330705c028104902f",
                    "54565de8c4c3a59714422c0df1fb68dad33b8f0cfe6cecdc31700072f4f97fc3",
                    "50d41a2b51a7e344cc7bae7a7b2ffa12a780884d7075dad826c50b862a7a24bf",
                    "53fa4eba45f8c40b624f8f20c1528506decb5e6eb544524380e4fdbd99323e83",
                    "a0807c9ab55ab21ef8430b65f0858e9770811786457af177adca8b1f77480494",
                    "1e5ceb8dd4dee165f29040f939d4d441bb33ca26b71c9aaa72dab88ba336f3e6",
                    "f323de584d80cc9f34cc782387727e19412a28c563d79e42697a017114c734eb",
                    "95cc664b58cdff76e462e46f63243bc8b3fc8e1c690edff55bb4eea61f7b0072",
                    "31bc86b68b39f546a16b05b2154d111c95f885fd866fec47712e382f38aeaa7d",
                    "be5bb66310d357d82f77c84f50fb9bd223401f68065e97727f9ca389631ae78a",
                    "4255a9b7f00595e8602349c1e1beb3e5ef6166bc37158aa35cabd853ee4b3f91",
                ],
                "c18e8e1f3c4f90fa288b6d75c1e38d14015cd1f61a5e70c9ce04dd3f5a3a65f6",
                vec![
                    "21210b61f97c78fefd4c3a266e28f9ce1f43e0a37b0ed8aa945b352c3bc55cd4",
                    "0a57caaa7d01f5dce091a38a782324b63e4d3ebb2346f7f16e5e23263f7efbac",
                    "ea5b73706e2dfbbf5797ce67b8647f9e31da9506f61a81a687300139477b20d7",
                    "29490422e4f092c19700b22869a470f0d450cc6530cc4ab523395ee280879adb",
                    "a8743e322199cca0df557135ac0b8f57e5a08ca1abedf60f2213ae4f78a46c75",
                    "dc734fc41ec0ba1d06f38c6d0992e5e8f78cb62020883c9c70de1838534b4bc8",
                    "6e34df79e2ede828b4d2aa0a9aebe4367fb17dd559f1c3fe83742b2c4f92c58b",
                    "e5c235583dffca60f513d6d22a4d194fdf793dd437bfd711ead22cc3d820b13e",
                    "7b6513591c542363414b45daf4d1d3712f13a5d6f0235f6b85ed6abeeeffa693",
                    "a3266edb545befeb0b8ac279b5ba3ad89d972b2bdda291b2997f17b6c5661998",
                    "961d1d6e48e8bce1a1a043b35c1d2905f56b729f9517131039ab49065ce42f08",
                    "6d89090a59fc0a1095cdb39718d91c21a387dbe3f1313d203cab9f3fbb6ea71a",
                ],
                vec![
                    "0b037f75d0abbb62cb4c9d571f26f075e42edb05efe0d9d59cf2d600325c612e",
                    "535dc69fa98c3ce925f98c4b99d61b4b6997047fce4399d97e8118e05c1b0df9",
                    "393dfb572ace77de34d88b395bd451e449d59b4e236a12f287d216fe677954ec",
                    "88178bdd5e0b842d3865fe3f9bbc13fceb89daed31c1e0ed75aebc54784ae28d",
                    "60df4537b944662e14f11e33342c00eaffb0fb58cc1f072485c556d435688dd0",
                    "23bc9f3861423821fea12ee670332bada3821b94a274504513aeb3a33ba207dc",
                    "093f5e237fa6e02486c433357bbaa50b88257b5fb282584ddba968fd838a764e",
                    "00b2e2a9d33232e329e6628a1f75edb6c2da970ebba684828b593522e8a990b1",
                    "132b6cc34895d9499f3102bd33cd383c190fe3658c996699fba9fa6507864a32",
                    "734e271dcf4dc1abe294bff74104c04203ac69c882f53e0ce7a36ebeb30b45b5",
                    "8a30e46165c9353ab647722059897dc3135176bca89096e33534b3fcded80308",
                    "b673d5e0d0bac20d69634952b66c46bbd34ff3956bdd336f98300abfdf1195cf",
                    "899d2f32c1a24b1ec33dd88c20ba7851f37de393e63ba9a8332549c35e81607b",
                    "88394a4873400f73aa146321f8e06ea22c419ef5c50c9c79d7dd58551dc6f1f2",
                    "1d584a41c4622da8a3d5e69ec22fcc2017d84ff2af5b0683aaf28f64daa0f6b8",
                    "646f35779ca66eb3ebecd79e9622902ce975fe273ddc94949e3f6868b225f4c6",
                    "ee72a5253128d5e5ca2a35af684811e40731db35c0918e097234a7c9972b5abe",
                    "204afec76428b95c5a9ca64a6143ad1a8e154490bbda86eef7d96ee86d023599",
                    "42afbbb145391faee34d2c00edd4f7144827b40f8ba061efb95f9c634aa2d05e",
                    "0ff937dca7ae3d360adaabf0772b0ab37c1da2aeec3dc73a5089982843442b05",
                    "e2684aab9f194d63276a32c3ef90841dc8a74edeae77db55eaaa44b47e201c2b",
                    "47ec05e2281feb95f66cb5c102a0860e745e55a1dadbb8bdbb3c431708b23389",
                    "759ee93542d144cc3a877acb86f573e480950e8e2d542d273c19a1f7944798ee",
                    "e646ceb03aee4bad78a8fc67e40f083cf123ef204e09e75ab4b788cf444beb0e",
                    "5db2957347add681eaf02698ca052d9febca5c2c5574f2f3083edb30cbd6e384",
                    "d42f713380c3122ffcd95839f799217a4899f1846159aede8603247f1a59aa88",
                    "97bfc57289e7de8f88bb5bae4a5ad4a6d77344109004385a46858136ccacd9be",
                    "5eaf70a56b80448cf0acec74288a8924137a834873b16429f9c542bf9225e2e7",
                    "b8cbebcd4a856177eeb99c0b85b97234bdf73cf8bf953e5e7cd9375b42eb3dd4",
                ],
            ),
        ];

        for (keys, values, root, query_keys, sibling_hashes) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data).unwrap();

            assert_eq!(**result.lock().unwrap(), hex::decode(root).unwrap());

            let proof = tree
                .prove(
                    &mut db,
                    &query_keys
                        .iter()
                        .map(|k| hex::decode(k).unwrap())
                        .collect::<NestedVec>(),
                )
                .unwrap();
            assert_eq!(
                proof
                    .sibling_hashes
                    .iter()
                    .map(hex::encode)
                    .collect::<Vec<String>>(),
                sibling_hashes
            );
            assert!(SparseMerkleTree::verify(
                &query_keys
                    .iter()
                    .map(|k| hex::decode(k).unwrap())
                    .collect::<NestedVec>(),
                &proof,
                &result.lock().unwrap(),
                KeyLength(32)
            )
            .unwrap());

            let query_keys_arr = query_keys
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>();
            let removed_keys = query_keys_arr[0..query_keys.len() / 2].to_vec();
            let new_keys = query_keys_arr[query_keys.len() / 2..].to_vec();
            let new_proof = SparseMerkleTree::remove_keys_from_proof(
                &proof,
                &removed_keys
                    .iter()
                    .map(|x| x.as_slice())
                    .collect::<Vec<_>>(),
            )
            .unwrap();

            assert!(SparseMerkleTree::verify(
                &new_keys,
                &new_proof,
                &result.lock().unwrap(),
                KeyLength(32)
            )
            .unwrap());
        }
    }

    #[test]
    fn test_small_proof() {
        let test_data =
            vec![
                (
                    vec![
                        "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                        "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                        "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                        "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                        "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                        "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                        "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                        "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                    ],
                    vec![
                        "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                        "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                        "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                        "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                        "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                        "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                        "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                        "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                        "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                        "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                    ],
                    "248c25930001a47d344e6807f25edad5195d71ad9c96b73a99cccc7906343112",
                    vec!["6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"],
                    vec![
                        "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                        "17b03e8f6995405f962b0dc01fdcbdaa523b7de7959c0c084c4355031e908a44",
                        "3d070fcf247a691bd2d792313f8934f6c37e2444c2d0be0cbc5096edc7b2a674",
                        "8df2cd060f0e84a9a19ae6a331f93ebcc49e5d513303c313a116ca1cd862bafa",
                    ],
                    vec![QueryProof {
                        bitmap: Arc::new(hex::decode("17").unwrap()),
                        pair: Arc::new(KVPair(
                            hex::decode(
                                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                            )
                            .unwrap(),
                            hex::decode(
                                "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                            )
                            .unwrap(),
                        )),
                    }],
                ),
                (
                    vec![
                        "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
                        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
                        "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
                        "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
                        "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
                        "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
                        "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                        "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
                        "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                        "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
                        "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
                        "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
                        "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                        "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
                        "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                        "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                        "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                        "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
                        "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
                        "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
                        "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
                        "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                        "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
                        "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                        "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                        "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
                        "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
                    ],
                    vec![
                        "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
                        "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                        "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
                        "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
                        "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
                        "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                        "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
                        "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
                        "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                        "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
                        "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                        "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
                        "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
                        "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
                        "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                        "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
                        "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                        "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                        "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                        "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
                        "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
                        "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
                        "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
                        "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                        "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
                        "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                        "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
                        "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
                        "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
                    ],
                    "2770cbf448782d4a24a53135741e1c7653d065c53e2cf150d431aad77993e2a0",
                    vec![
                        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    ],
                    vec![
                        "7aada8d512d761dcd18031059d1c17d80d9c79f67ae3babf5cabf260d5790cae",
                        "7fd4719a219a224ecd4f1ef19eac8ccbc244a7368857707deb3b65c351c0c0a7",
                        "93b98c20a3b72d7aee973c2374d95db8a79912be7a4641dfc28f26d441bf7890",
                        "44195ef71f7a750609cd0edcfbce505092216d06b4bffa52607eb2770d72a9a7",
                        "e8527ba9c82736c6a644049a88e4a0c7eabbb23adf20d0dd40b7f8d474642a27",
                        "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                        "32bb08752a5dd3a88d168dc52b3e7c53a89a6679b23ceb2c5c26a7af495bd128",
                        "0498d41cc43f5ee1be71781513164d2dfea38bf0ee3289f02d18127f7c4406d6",
                        "f03feb21e428c640620dc2698787943c5d67b895ff316594bdd36493d2deca2b",
                        "03662db407793a42f74699970ea28cce39304449eb900d15003a2b79bb9d1e7b",
                        "f4fba601b5a1624731f3c4153defe761f977d167ea5a77a110dd571cbcd9bc2a",
                        "884d43dbcd182a451954f4633481126ab414e9650a3743203922e528228883a4",
                    ],
                    vec![
                    QueryProof {
                        bitmap: Arc::new(hex::decode("3f").unwrap()),
                        pair: Arc::new(KVPair(hex::decode(
                            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        )
                        .unwrap(),
                        hex::decode(
                            "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                        )
                        .unwrap(),)),
                    },
                    QueryProof {
                        bitmap: Arc::new(hex::decode("bf").unwrap()),
                        pair: Arc::new(KVPair(hex::decode(
                            "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        )
                        .unwrap(),
                        hex::decode(
                            "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                        )
                        .unwrap(),)),
                    },
                    QueryProof {
                        bitmap: Arc::new(hex::decode("2f").unwrap()),
                        pair: Arc::new(KVPair(hex::decode(
                            "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                        )
                        .unwrap(),
                        hex::decode(
                            "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                        )
                        .unwrap(),)),
                    },
                ],
                ),
            ];

        for (keys, values, root, query_keys, sibling_hashes, queries) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data).unwrap();

            assert_eq!(**result.lock().unwrap(), hex::decode(root).unwrap());

            let proof = tree
                .prove(
                    &mut db,
                    &query_keys
                        .iter()
                        .map(|k| hex::decode(k).unwrap())
                        .collect::<NestedVec>(),
                )
                .unwrap();
            for (i, query) in proof.queries.iter().enumerate() {
                assert_eq!(query.bitmap, queries[i].bitmap);
                assert_eq!(query.key(), queries[i].key());
                assert_eq!(query.value(), queries[i].value());
            }
            assert_eq!(
                proof
                    .sibling_hashes
                    .iter()
                    .map(hex::encode)
                    .collect::<Vec<String>>(),
                sibling_hashes
            );
            assert!(SparseMerkleTree::verify(
                &query_keys
                    .iter()
                    .map(|k| hex::decode(k).unwrap())
                    .collect::<NestedVec>(),
                &proof,
                &result.lock().unwrap(),
                KeyLength(32)
            )
            .unwrap());
        }
    }

    #[test]
    fn test_mid_proof() {
        let test_data = vec![(
            vec![
                "58f7b0780592032e4d8602a3e8690fb2c701b2e1dd546e703445aabd6469734d",
                "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                "4d7b3ef7300acf70c892d8327db8272f54434adbc61a4e130a563cb59a0d0f47",
                "265fda17a34611b1533d8a281ff680dc5791b0ce0a11c25b35e11c8e75685509",
                "77adfc95029e73b173f60e556f915b0cd8850848111358b1c370fb7c154e61fd",
                "9d1e0e2d9459d06523ad13e28a4093c2316baafe7aec5b25f30eba2e113599c4",
                "beead77994cf573341ec17b58bbf7eb34d2711c993c1d976b128b3188dc1829a",
                "9652595f37edd08c51dfa26567e6cd76e6fa2709c3e578478ca398d316837a7a",
                "cdb4ee2aea69cc6a83331bbe96dc2caa9a299d21329efb0336fc02a82e1839a8",
                "452ba1ddef80246c48be7690193c76c1d61185906be9401014fe14f1be64b74f",
                "3973e022e93220f9212c18d0d0c543ae7c309e46640da93a4a0314de999f5112",
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "8a5edab282632443219e051e4ade2d1d5bbc671c781051bf1437897cbdfea0f1",
                "684888c0ebb17f374298b65ee2807526c066094c701bcc7ebbe1c1095f494fc1",
                "83891d7fe85c33e52c8b4e5814c92fb6a3b9467299200538a6babaa8b452d879",
                "dc0e9c3658a1a3ed1ec94274d8b19925c93e1abb7ddba294923ad9bde30f8cb8",
                "bd4fc42a21f1f860a1030e6eba23d53ecab71bd19297ab6c074381d4ecee0018",
                "ef6cbd2161eaea7943ce8693b9824d23d1793ffb1c0fca05b600d3899b44c977",
                "4a64a107f0cb32536e5bce6c98c393db21cca7f4ea187ba8c4dca8b51d4ea80a",
                "951dcee3a7a4f3aac67ec76a2ce4469cc76df650f134bf2572bf60a65c982338",
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                "36a9e7f1c95b82ffb99743e0c5c4ce95d83c9a430aac59f84ef3cbfab6145068",
                "f299791cddd3d6664f6670842812ef6053eb6501bd6282a476bbbf3ee91e750c",
                "32ebb1abcc1c601ceb9c4e3c4faba0caa5b85bb98c4f1e6612c40faa528a91c9",
                "2b4c342f5433ebe591a1da77e013d1b72475562d48578dca8b84bac6651c3cb9",
                "bb7208bc9b5d7c04f1236a82a0093a5e33f40423d5ba8d4266f7092c3ba43b62",
                "68aa2e2ee5dff96e3355e6c7ee373e3d6a4e17f75f9518d843709c0c9bc3e3d4",
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                "334359b90efed75da5f0ada1d5e6b256f4a6bd0aee7eb39c0f90182a021ffc8b",
                "09fc96082d34c2dfc1295d92073b5ea1dc8ef8da95f14dfded011ffb96d3e54b",
                "8a331fdde7032f33a71e1b2e257d80166e348e00fcb17914f48bdb57a1c63007",
                "67586e98fad27da0b9968bc039a1ef34c939b9b8e523a8bef89d478608c5ecf6",
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                "e7cf46a078fed4fafd0b5e3aff144802b853f8ae459a4f0c14add3314b7cc3a6",
                "ba5ec51d07a4ac0e951608704431d59a02b21a4e951acc10505a8dc407c501ee",
                "ffe679bb831c95b67dc17819c63c5090d221aac6f4c7bf530f594ab43d21fa1e",
                "d03502c43d74a30b936740a9517dc4ea2b2ad7168caa0a774cefe793ce0b33e7",
                "01ba4719c80b6fe911b091a7c05124b64eeece964e09c058ef8f9805daca546b",
                "8f11b05da785e43e713d03774c6bd3405d99cd3024af334ffd68db663aa37034",
                "a318c24216defe206feeb73ef5be00033fa9c4a74d0b967f6532a26ca5906d3b",
                "ca358758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879",
                "ab897fbdedfa502b2d839b6a56100887dccdc507555c282e59589e06300a62e2",
                "7cb7c4547cf2653590d7a9ace60cc623d25148adfbc88a89aeb0ef88da7839ba",
                "1f18d650d205d71d934c3646ff5fac1c096ba52eba4cf758b865364f4167d3cd",
                "2f0fd1e89b8de1d57292742ec380ea47066e307ad645f5bc3adad8a06ff58608",
                "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                "bbf3f11cb5b43e700273a78d12de55e4a7eab741ed2abf13787a4d2dc832b8ec",
                "c555eab45d08845ae9f10d452a99bfcb06f74a50b988fe7e48dd323789b88ee3",
            ],
            vec![
                "1de48a4dc23d38868ea10c06532780ba734257556da7bc862832d81b3de9ed28",
                "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                "d6cdf7c9478a78b29f16c7e6ddcc5612e827beaf6f4aef7c1bb6fef56bbb9a0f",
                "e368b5f8bac32462da14cda3a2c944365cbf7f34a5db8aa4e9d85abc21cf8f8a",
                "fd2e24dccf968b46e13c774b139bf8ce13c74c58713fe25ab752b9701c6de8f9",
                "e17d630e7b1ec8612c95f2a37755c70466640272a6aee967e16239f2c66a81d4",
                "42bbafcdee807bf0e14577e5fa6ed1bc0cd19be4f7377d31d90cd7008cb74d73",
                "2651c51550722c13909ec43f50a1da637f907f7a307e8f60695ae23d3380abad",
                "8a1fe157beac6df9db1f519afed60928eeb623c104a53a62af6b88c423d47e35",
                "c2908410ab0cbc5ef04a243a6c83ee07630a42cb1727401d384e94f755e320db",
                "fc62b10ec59efa8041f5a6c924d7c91572c1bbda280d9e01312b660804df1d47",
                "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                "e344fcf046503fd53bb404197dac9c86405f8bd53b751e40bfa0e386df112f0f",
                "ff122c0ea37f12c5c0f330b2616791df8cb8cc8f1114304afbf0cff5d79cec54",
                "d703d3da6a87bd8e0b453f3b6c41edcc9bf331b2b88ef26eb39dc7abee4e00a3",
                "be81701528e54129c74003fca940f40fec52cbeeaf3bef01dc3ff14cc75457e4",
                "6ba6a79b31adb401532edbc80604b4ba490d0df9874ac6b55a30f91edfd15053",
                "0eac589aa6ef7f5232a21b36ddac0b586b707acebdeac6082e10a9a9f80860da",
                "3b7674662e6569056cef73dab8b7809085a32beda0e8eb9e9b580cfc2af22a55",
                "bf1d535d30ebf4b7e639721faa475ea6e5a884f6468929101347e665b90fccdd",
                "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                "24944f33566d9ed9c410ae72f89454ac6f0cfee446590c01751f094e185e8978",
                "d25c96a5a03ec5f58893c6e3d23d31751a1b2f0e09792631d5d2463f5a147187",
                "d8909983be3179a28734edac2ad9e1d0364c8e15e6e8cdc1363d9969d23c7d95",
                "2ad16b189b68e7672a886c82a0550bc531782a3a4cfb2f08324e316bb0f3174d",
                "7e0e5207f9102c79bd355ccafc329b517e0d6c2b509b37f30cfc39538992cb36",
                "e4e6a38d6bcce0067eedfb3343a6aaba9d42b3f3f71effb45abe5c4a35e337e8",
                "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                "7c8b1ed7e1074189bf7ff3618e97b4ce88f25c289f827c30fabfbe6607058af6",
                "d4880b6be079f51ee991b52f2e92636197de9c8a4063f69987eff619bb934872",
                "ef79a95edac9b7119192b7765ef48dc8c7ecc0db12b28d9f39b5b4dedcc98ccd",
                "f3035c79a84a2dda7a7b5f356b3aeb82fb934d5f126af99bbee9a404c425b888",
                "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                "92a9cee8d181100da0604847187508328ef3a768612ec0d0dcd4ca2314b45d2d",
                "2ec9b3cc687e93871f755d6c9962f62f351598ba779d9838aa2b68c8ef309f50",
                "084aa2e4bc2defcd2c409c1916023acd6972624cf88112280b6dacde48367b0c",
                "0ca5765ffb7eb99901483c2cda1dd0209cef517e96e962b8c92c1668e5334d43",
                "9c827201b94019b42f85706bc49c59ff84b5604d11caafb90ab94856c4e1dd7a",
                "1bb631b04e6dce2415d564c3ebcd43d6d8baef041f00f9423600e134d2df634d",
                "0272614c70432bc1f94b8739603ba170ad5f4866a9936f37c463767bda7d005d",
                "b6d58dfa6547c1eb7f0d4ffd3e3bd6452213210ea51baa70b97c31f011187215",
                "58b8e1205472ebed51a76303179ebf44554714af49ef1f78fb4c1a6a795aa3d7",
                "cf29746d1b1686456123bfe8ee607bb16b3d6e9352873fd34fd7dfc5bbfb156c",
                "a2215262d04d393d4e050d216733138a4e28c0b46375e84b7a34a218db8dc856",
                "017e6d288c2ab4ed2f5e4a0b41e147f71b4a23a85b3592e2539b8044cb4c8acc",
                "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                "247f88a674f9f504e95f846272b120deaa29b0ae6b0b9069689488ba3c8e90ab",
                "1405870ede7c8bede02298a878e66eba9e764a1ba55ca16173f7df470fb4089d",
            ],
            "401dba08d0bc4edf4d1f816742b59385f6617e156904d6bee7e9b4f096abf847",
            vec![
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
            ],
            vec![
                "ccc998cd27d1c7c74de713dc9e31fed4c46ecb7375c9fc681e157d1bcb413bf3",
                "8155096662f2bbde82e17684a0a0d1c4afcd316f3b45731e07ccd6cd69d1ee4f",
                "7aada8d512d761dcd18031059d1c17d80d9c79f67ae3babf5cabf260d5790cae",
                "7fd4719a219a224ecd4f1ef19eac8ccbc244a7368857707deb3b65c351c0c0a7",
                "9b5c0979c8f3b7d7651edc5532435014b5beaa980cfd9cab3fdcae36d9df4b00",
                "44195ef71f7a750609cd0edcfbce505092216d06b4bffa52607eb2770d72a9a7",
                "74b28dfde1df60fa62e78609e0b7c457d91bbe732ff82737d4297085ba12c866",
                "e8527ba9c82736c6a644049a88e4a0c7eabbb23adf20d0dd40b7f8d474642a27",
                "be28d7c49764a8673dffc61a8f9df827e7f296f03fc91efdebcb3582f6de1e66",
                "2303f37ce9621f2ce5f3c046c95cdf221fdd53e01d97aaa339c7c06521cf007f",
                "47da630f0a89f184eb1d296a3f17cea1a750e1c7c1a3b1ef2f4f60034d64c9fe",
                "ff69738cfb9678d0677b35780f8c5d18e2e59725632c642fa5ac4f7baf6e9f32",
                "32bb08752a5dd3a88d168dc52b3e7c53a89a6679b23ceb2c5c26a7af495bd128",
                "0498d41cc43f5ee1be71781513164d2dfea38bf0ee3289f02d18127f7c4406d6",
                "b33bb9602edceb6854bd539ee77bec8bea656ce86b62be30bdee43876d3ebee2",
                "caefa0f9757b2c6820e34c43a48133d86207ac53abcae2be34f4bb34a31bc855",
                "f856f0a29502b912714605395a6394373877e5ffdc544bc5477054866b88f16e",
                "9636e2b9312e0ae556db5a7c926a82e7597764d40059bd796c3ae215e8ed1e3d",
            ],
            vec![
                QueryProof {
                    bitmap: Arc::new(hex::decode("3f").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        )
                        .unwrap(),
                        hex::decode(
                            "1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a",
                        )
                        .unwrap(),
                    )),
                },
                QueryProof {
                    bitmap: Arc::new(hex::decode("bf").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        )
                        .unwrap(),
                        hex::decode(
                            "9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70",
                        )
                        .unwrap(),
                    )),
                },
                QueryProof {
                    bitmap: Arc::new(hex::decode("3f").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                        )
                        .unwrap(),
                        hex::decode(
                            "1cc3adea40ebfd94433ac004777d68150cce9db4c771bc7de1b297a7b795bbba",
                        )
                        .unwrap(),
                    )),
                },
                QueryProof {
                    bitmap: Arc::new(hex::decode("9f").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                        )
                        .unwrap(),
                        hex::decode(
                            "c942a06c127c2c18022677e888020afb174208d299354f3ecfedb124a1f3fa45",
                        )
                        .unwrap(),
                    )),
                },
                QueryProof {
                    bitmap: Arc::new(hex::decode("5f").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                        )
                        .unwrap(),
                        hex::decode(
                            "214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7",
                        )
                        .unwrap(),
                    )),
                },
                QueryProof {
                    bitmap: Arc::new(hex::decode("015f").unwrap()),
                    pair: Arc::new(KVPair(
                        hex::decode(
                            "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
                        )
                        .unwrap(),
                        hex::decode(
                            "88e443a340e2356812f72e04258672e5b287a177b66636e961cbc8d66b1e9b97",
                        )
                        .unwrap(),
                    )),
                },
            ],
        )];

        for (keys, values, root, query_keys, sibling_hashes, queries) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }
            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data).unwrap();

            assert_eq!(**result.lock().unwrap(), hex::decode(root).unwrap());

            let proof = tree
                .prove(
                    &mut db,
                    &query_keys
                        .iter()
                        .map(|k| hex::decode(k).unwrap())
                        .collect::<NestedVec>(),
                )
                .unwrap();
            for (i, query) in proof.queries.iter().enumerate() {
                assert_eq!(query.bitmap, queries[i].bitmap);
                assert_eq!(query.key(), queries[i].key());
                assert_eq!(query.value(), queries[i].value());
            }
            assert_eq!(
                proof
                    .sibling_hashes
                    .iter()
                    .map(hex::encode)
                    .collect::<Vec<String>>(),
                sibling_hashes
            );
            assert!(SparseMerkleTree::verify(
                &query_keys
                    .iter()
                    .map(|k| hex::decode(k).unwrap())
                    .collect::<NestedVec>(),
                &proof,
                &result.lock().unwrap(),
                KeyLength(32)
            )
            .unwrap());
        }
    }

    #[test]
    fn test_update_data_new_from() {
        let mut cache = Cache::new();
        cache.insert(vec![1, 2, 3], vec![4, 5, 6]);

        let mut data = UpdateData::new_from(cache);
        assert_eq!(data.data.get(&vec![1, 2, 3]).unwrap(), &vec![4, 5, 6]);

        data.insert(SharedKVPair(&[7, 8, 9], &[10, 11, 12]));
        assert_eq!(data.data.get(&vec![7, 8, 9]).unwrap(), &vec![10, 11, 12]);
    }

    #[test]
    fn test_query_proof_with_proof() {
        let pair = Arc::new(KVPair(
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
            hex::decode("214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7")
                .unwrap(),
        ));
        let binary_bitmap = [
            true, false, false, false, true, false, true, true, false, true,
        ];
        let ancestor_hashes = vec![
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
        ];
        let sibling_hashes = vec![
            hex::decode("084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5")
                .unwrap(),
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
        ];
        let proof = QueryProofWithProof::new_with_pair(
            pair,
            &binary_bitmap,
            &ancestor_hashes,
            &sibling_hashes,
        );

        assert_eq!(proof.binary_bitmap, binary_bitmap);
        assert_eq!(proof.ancestor_hashes, ancestor_hashes);
        assert_eq!(proof.sibling_hashes, sibling_hashes);
        assert_eq!(
            proof.hash,
            vec![
                26, 170, 13, 17, 56, 228, 164, 98, 185, 10, 145, 76, 19, 139, 229, 210, 43, 84,
                250, 5, 200, 32, 49, 85, 172, 216, 46, 204, 182, 218, 185, 238
            ]
        );

        assert_eq!(proof.height(), 10);
        assert!(!proof.is_zero_height());
    }

    #[test]
    fn test_query_proof_with_proof_slice_bitmap() {
        let pair = Arc::new(KVPair(
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
            hex::decode("214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7")
                .unwrap(),
        ));
        let binary_bitmap = [
            true, false, false, false, true, false, true, true, false, true,
        ];
        let ancestor_hashes = vec![
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
        ];
        let sibling_hashes = vec![
            hex::decode("084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5")
                .unwrap(),
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
        ];
        let mut proof = QueryProofWithProof::new_with_pair(
            pair,
            &binary_bitmap,
            &ancestor_hashes,
            &sibling_hashes,
        );

        proof.slice_bitmap();
        assert_eq!(
            proof.binary_bitmap,
            [false, false, false, true, false, true, true, false, true]
        );

        proof.slice_bitmap();
        assert_eq!(
            proof.binary_bitmap,
            [false, false, true, false, true, true, false, true]
        );

        proof.slice_bitmap();
        assert_eq!(
            proof.binary_bitmap,
            [false, true, false, true, true, false, true]
        );

        proof.slice_bitmap();
        assert_eq!(proof.binary_bitmap, [true, false, true, true, false, true]);

        proof.slice_bitmap();
        proof.slice_bitmap();
        proof.slice_bitmap();
        proof.slice_bitmap();
        proof.slice_bitmap();
        assert_eq!(proof.binary_bitmap, [true]);
    }

    #[test]
    fn test_query_proof_with_proof_binary_path() {
        let pair = Arc::new(KVPair(
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
            hex::decode("214e63bf41490e67d34476778f6707aa6c8d2c8dccdf78ae11e40ee9f91e89a7")
                .unwrap(),
        ));
        let binary_bitmap = [
            true, false, false, false, true, false, true, true, false, true,
        ];
        let ancestor_hashes = vec![
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
            hex::decode("e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71")
                .unwrap(),
        ];
        let sibling_hashes = vec![
            hex::decode("084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5")
                .unwrap(),
            hex::decode("120d7bda8c955120c64f80f242c171a778d718b62b4f6376d6d6acfc7cb0b8").unwrap(),
        ];
        let proof = QueryProofWithProof::new_with_pair(
            pair,
            &binary_bitmap,
            &ancestor_hashes,
            &sibling_hashes,
        );

        let path = proof.binary_path();
        assert_eq!(
            path,
            vec![true, true, true, false, false, true, false, true, false, false]
        );
    }

    #[test]
    fn test_bitmap_len_with_verify() {
        let mut data = UpdateData::new_from(Cache::new());
        let keys = ["bbbbc758f6d27e6cf45272937977a748fd88391db679ceda7dc7bf1f005ee879"];
        let values = ["9c12cfdc04c74584d787ac3d23772132c18524bc7ab28dec4219b8fc5b425f70"];

        for i in 0..keys.len() {
            data.insert(SharedKVPair(
                &hex::decode(keys[i]).unwrap(),
                &hex::decode(values[i]).unwrap(),
            ));
        }

        let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
        let mut db = smt_db::InMemorySmtDB::default();
        let root = tree.commit(&mut db, &data).unwrap();

        let mut proof = tree
            .prove(
                &mut db,
                &keys
                    .iter()
                    .map(|k| hex::decode(k).unwrap())
                    .collect::<NestedVec>(),
            )
            .unwrap();

        // length = 32
        proof.queries[0].bitmap = Arc::new(vec![
            1u8, 2, 3, 4, 1, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71,
            3, 3, 71, 3, 3, 71,
        ]);
        assert!(!SparseMerkleTree::verify(
            &keys
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>(),
            &proof,
            &root.lock().unwrap(),
            KeyLength(32),
        )
        .unwrap());

        // length = 33
        proof.queries[0].bitmap = Arc::new(vec![
            1u8, 2, 3, 4, 1, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71, 3, 3, 71,
            3, 3, 71, 3, 3, 71, 3,
        ]);
        let res = SparseMerkleTree::verify_and_prepare_proof_map(
            &proof,
            &keys
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>(),
            KeyLength(32),
        );
        assert_eq!(res.unwrap_err(), SMTError::InvalidBitmapLen);
    }

    #[test]
    fn test_verify_and_prepare_proof_map() {
        let test_data = vec![(
            vec![
                "0a43663899d5e3697fbe30e0ed03bc94077651bf267316034791f9d74a880efd",
                "5c5a63ea59240410d291571f26b7b082c645e9935b572cacbb01f2598d9e650a",
                "6c335694ff8ed2a665f30afa954d2cca11d779fa31df32988eca82f4b863825e",
                "5eeaacb199069a7efae65e6e2660e6e4c40f034f3507c6307fa419bda2f89b68",
                "f4eec52aac2504b97a2a074fe7a30d35f8ea4a93eef335b665066d9badfc950a",
                "25ddb8af44e3222276a491ff1cbd6781c324bd59c3ccfc3f3168c96ffbed83ce",
                "2ae39e138cc695cceef4fd17bf3f1ef399064e2a45d70578a0ce21442030d5aa",
                "f1e467327b7434c610e552337a5b133dbafa4187c90a904ffce143cb3c3ef1c5",
                "f57b0ad0f1a34bb4e86accbec02a510ff3eeb01227ff6a9c470988c76ec6c3ee",
                "c16999fded3e396869e55dbf7744e9df1500bdb97e79eedc4b7d97f4448e20b6",
                "29490422e4f092c19700b22869a470f0d450cc6530cc4ab523395ee280879adb",
                "5fec2e461402697c00ac20f73a7397b434dc4535d2c0cc159f84a06899d32414",
                "a423e7cb2384e091467276b310536a83d8bb325a71597f9900d73a52c82c7137",
                "9d615bad9eca3e22ccd4e8947395d56acb41966950a3944f4cfbb467a0b1c19b",
                "961d1d6e48e8bce1a1a043b35c1d2905f56b729f9517131039ab49065ce42f08",
                "14753f55200fd793843e4ad744c1254c4c2ae96882389f2244a8b2870953cce2",
                "3285f3fbf0f80447f3da1145e468d4f570313e50a94a83565099b777624111bd",
                "9c8e90a2b87e6b0de76ebd8a7473804466230a8bb1a1d010dc95ee12f528724b",
                "e5c235583dffca60f513d6d22a4d194fdf793dd437bfd711ead22cc3d820b13e",
                "fa368b8c5d4d85883c965ca5c0ba8ad794df670a7fb56ec7de711f3618989430",
                "7c14ac8b656212a6c31e83ebbaf9db1cad3475fb8503ce6230d513663fdc2953",
                "0a57caaa7d01f5dce091a38a782324b63e4d3ebb2346f7f16e5e23263f7efbac",
                "872d3d8070403e1b9503ca4f88b3659f2fb51ee10aa8f33143b5ded8fcc2b9fd",
                "83013cf7a3674486b8dbbf4137e40c5f190baa5c1f6ae434a1489403eef09dfb",
                "1468b7e16757ea8a35f7ef0634c6b86ba62520036d336be6a76fefa7dc5576a6",
                "50fe1fd6dc177ada292e26a5a14037dc3cdeb7deaaf41983fa754f1b4bd92d21",
                "49bb88f9c75737f63fee583044ec1d17f3a108872f368d65bdb5be0e43e01076",
                "df5d8ed7b94f3fd9851f0a0ea8a4626db32c0508a57d29a2527fcc3773dbf94d",
                "d51f73ae1499f2a67c8edcb6506587ade45f2919b91a6c2f7e510daee7e26cc2",
                "9cd4d919e1036f8e4f89c0531c1d5220f9015a61b3377bbdfb0470dc5ed0480f",
                "7a849a6b8db34092cb02b077986023ed23bf8ce53e3796fc9f95207a6cbef43d",
                "d0e73e5cb3150e84ac3520f5a9bccd225f4a2214801ae504c060a977c3f25add",
                "3f9cc2ccbc7fa149620c9c8ed3e02b8fb41a878f236acdba3f5899694df63a4d",
                "da747de7a5aa7151e76a6e5cdc72418476aefd3f5bd7ed2ec34871e91056be54",
                "7bd2b56d98722c417ff3a36db1eece25d1f7f16498a920775c570aaec11042bb",
                "da25a80d9095064c7b8168cd12c98ee3e046cc7824e7a020cc2d710f236de2e9",
                "62c92c73a88528825f056071b487a6a270c8a4a387ce12724b2a5c984fe4e39e",
                "11f81d81c489ac1de76893e1cae82a66672d4b78ddc62c27af824181a832d6ad",
                "6d89090a59fc0a1095cdb39718d91c21a387dbe3f1313d203cab9f3fbb6ea71a",
                "778d23b624c2a606cd4156d4b9e87f4b963a863197e720f1aabb73a4d06e6845",
                "571513a9b592ab83704f8b21cd3f1134cdb6cb3496aed3e677e61abf1971cf0d",
                "45ba72128026896228a919e34e263ce336475b6d2724fdaa632c699b20bf0cea",
                "062adc058e9af3fef721c8373e9f6047181b9f984e660554ac66d3afa149bb77",
                "a4d8d370e108977a01739ce8ea8957f585005d75fd50d4f30508db3c59156db8",
                "990ba63ca2f991a5ccf24c8c1cc3b7ab596fb39c350c3bddc2808d826206cdbe",
                "6a5127d4d0f2ee5e6b87cd5a39e1ee215bf8d17009bcec96d88828a5321e1be4",
                "124efe57d873d27930b5ca20dc0606ac106c9415d8f5fe0501b93327edee27fd",
                "2caf522da31dc339e54c7de7089682280ab8d1646bf46b7c48591d948878fb5b",
                "d063520eac73b4eeddb354676fe919a089bc1573670a0929cf9befc5ee8f357f",
                "0cc0fddd53fdc7170d1120de7f503d4d65abff979ced982293ef50f2ccd6e226",
                "ba3a5cd72eb6513d7ab8d5b0c72727fcb385692ea26ee6cb0a63f227e8ebd734",
                "23eef2af3b7d9869545629a6f93b621d4c6790f5abae48bd70b80b7332b39fb5",
                "d6a5a8bc3c2e628a481683dcb4f718bc6d7626d9d2e7b6d1e4b5450c572e6d9f",
                "984b8b67f8cec9967ce207a783e76d6731b4bc8d42b9f853be16ebf7827a6dd2",
                "321ee2dfdad92ac2229607994f5a9e1688e99116b3b210c67228a644ea894681",
                "e66becdfe96c1bca997eff67915bc8f0902d481221a60fb65d90a1048d3b7e97",
                "bb7d4c494c96f548e006f5339f9d558cb3cb0fe5541fa21032a196fe1426bb80",
                "43ad4e6bd96fe7af330bf9b84a5156b7343b0e00957ad1795c6cfdb705120486",
                "166fa383d238a5a004543c6b42c913b288d9ce314b4c984340ebfbb754415251",
                "8aa45598c7bd2468c2926528ee7c622f0356702bf7ba65b7e5b7f6c5be45422a",
                "48bb3d896ac98068d4976fe73c9723b870d342e091c56826169b7aa491a619de",
                "7ecf90725cc58e8fd203414a821fa7b2cf81302b4a97882cfe7927507eba7ff4",
                "a8743e322199cca0df557135ac0b8f57e5a08ca1abedf60f2213ae4f78a46c75",
                "301597d8ae03f46e36cc763dcca79add8dcb9d56f02215c84873a2d2a41db561",
                "dc734fc41ec0ba1d06f38c6d0992e5e8f78cb62020883c9c70de1838534b4bc8",
                "71b16968c679e9d25b37d4de174b3016b8588c5b6e06c9100f107144194be39a",
                "c5e9c83494ecb01d60c67686a6fec40cd3953845f67611004beb6b01abc74c7b",
                "b455086fcf84892566d9521b6fa539eaf29df9227e238af4941b9072fee68d8e",
                "1f66d008d5feee096868b568ffd916c3ec6644d155c0bb69fb169af674f18ebe",
                "785acf55f5ce0b2b815c21bc4f722cc9e23e878629db83a7a8f8bc8e117d32e1",
                "ebab7db679ee6b4552397bbc308b738a65504bb1b853bec18b7991ebdf7f2783",
                "c1e158bdf0d3fb3ccf1851390607a6a8a374f969ee4bddfada59d555f68b9a03",
                "2760c56cd58f0cb596b636e10cb96de60cfeda878cbde6567df5ad7eef83eee1",
                "77e0c81547a192fdf201027d106a25e3b67974f5818ff894498ce53c62a273ee",
                "b46c3de0490e0cba6f7aa3b0accddf77ee02b942e22f2dc13b4167e26ebd07bc",
                "52f38b71f5a9409ada100f7cb7b8736ec2abc04e9a43e1903672f61dcbbf18a6",
                "eb6f32c239fbec9ecd78209d4006048b9d170d0c1f2fb6192097d783ae1380ab",
                "4e1b9f11013aa6c54bd08fdc738b69f1cb5371e9774d9997517af0dc287fc52d",
                "5080242949956402863be82721e0c456835004df5059f71c4016f13f996dd498",
                "2f16168dcd74011db16616b4f18f4d0da6dd69bfb31bb345612c8b53a252e128",
                "21210b61f97c78fefd4c3a266e28f9ce1f43e0a37b0ed8aa945b352c3bc55cd4",
                "0cf38e5f4bae2e30f8f76e57407529c53ea383fa21c25436b91080df5f3f162a",
                "0e98ef871f756a4bc8afa1106e13ee8c2e8c311dbdf0364ba37252e73f97c719",
                "a3266edb545befeb0b8ac279b5ba3ad89d972b2bdda291b2997f17b6c5661998",
                "7b6513591c542363414b45daf4d1d3712f13a5d6f0235f6b85ed6abeeeffa693",
                "d8b7955739b03d2c13144814362ac1cf3263aea89b99ffd4d3c50aa2f3015931",
                "869c97475ba635a3d657df86b60aee0907281a780947da62e917c2f54f1e5182",
                "b6516aeb07e34f3a929270fcab0765aa7f093d43bbe0b2493603c6687447212a",
                "481863d4f2876ea972655649a94f941f3c0dfc3db504ef8dc558c8d396c2e321",
                "1bad5b320590c0b8b54d79188ea32cf918182858a355284c8260ff0400a54515",
                "b1f3c716d22c2c9add1f2ae93b9619aea36bdf1d375f822bc266f88969e29d4f",
                "ea5b73706e2dfbbf5797ce67b8647f9e31da9506f61a81a687300139477b20d7",
                "b84834ea9b3f77346387b946e2b5579e75e49b5ddf1358acb8a83ad0d2b4c8a0",
                "6e34df79e2ede828b4d2aa0a9aebe4367fb17dd559f1c3fe83742b2c4f92c58b",
                "d2faf70f2938836d9355c195a7e55899d1cd602950721087bd94002dca6b1994",
                "0130cea6b391aa2f84f9a8e1377882923deb97610b2aa9f7502fdf985be57c39",
                "9f1cfdfd776e6f880431570932147e83c854c4f9af3f3670ab36f2f85405d9fd",
                "2a6e6aeb2fb6fd95a81591fa61f6e7e5a70af1273d5324e03bb0d6d77b332f4c",
                "1ce7b6d7e06e988092ddb4d9e08ba11bf329c21d873f9a6cf3436482d657d88d",
                "6690f3bf1cd17516d4336de9e8a5dd3a036b559c3940230d5c779be4bd84d29e",
            ],
            vec![
                "4f6efe6f3ec6ea536f06082795ef06fc0ca0dc9702ce2131a7a605c5b274055f",
                "50b3720b3bafdcccee1096d6d602119abeca05c25503f8c92d74a760f8e0bca2",
                "2eb58b377bbc5bdc59715cda3013a609978b808834c8b99bb5434a7141788845",
                "7cc7756fbc0681f763f1d48a325832952a0d6b7f6bf1d7683b993c85ff8aa9ef",
                "7135af678793cf50eabb194599ff96d770949ad0655f2a8e9d23fc354b5f465b",
                "f993ff500beab3a9022d66222a3ede5273916f0d8fecfd9633d68c97bde00335",
                "9def97d1159e813336bed3c551edd5c9de8d9255dd69f5e8c97bea75ce7cba44",
                "2f3c6ea8723920c647cac4cee5f1ad1e1396583eb887032c7d513c3f27a9d681",
                "ed14ddcc17c4f3510a2672c5ed587c66f3b5bacc642b4f98b5dff197ae10de77",
                "ba060ebfe9f76cd0d0183d1ff2a505b8d46fc7894b4c816fa811a22f20e34b92",
                "2655cdc84676e20572b3d2814eb2e0364abd9c6ee0130594281cbb79f07b4a9d",
                "2535da7b9656307e542c36119255f9fde2a9d0df901dccb0a15dcd612957fec3",
                "182b98e61dfa6cd9b1ed29eb84151f40b6cb04dd632a594222b6f10e17b353fd",
                "d1190946ac63e646a3f6e6f256721e362829e73a87c877d11bd6fbac4218ac2d",
                "bdc8ea341a0d75a38dd0c281312be62e8767405363f6dcc3511dc056dd0c2311",
                "5eb43b960205e8ddacc043c44878f40a8bcf1aa57f2207f88ece3ad8f4ebf3b8",
                "10e732dd3592c26fdb9abbb96b2718c6587e550402e07ed7d6331caabcb26766",
                "f8e4990303eaa241a17dd7da695f18c2f409ed66a07b510dfcca499a45e7d0ec",
                "a2b598556cbd97ea392ab5e9f2740f5ddbe77175915c62e9dae2da366a39417d",
                "1e5ceb8dd4dee165f29040f939d4d441bb33ca26b71c9aaa72dab88ba336f3e6",
                "604ac5e1b0432637620bc528632f1e70443dadab09a55ca437b75ec33590e106",
                "2ebd50879e98349a20001c17efe31b007593c80716bcad83f6ee80447cf18574",
                "400a9fa42015e54b0996b406c7440e77f4cede0993300880b2f7960ec89b5e0d",
                "38f0357996c53c2d38cb988ef315b6f264855f150a45bb484ad3868164713483",
                "a872893a42fc61b0e4c60b8a17eb9e4311a86dd45d598e55a654539df361c0f6",
                "14fdbafce811d2444ea02c9b4d7dc4368ca6096a40bf1b410361c4625901d123",
                "3d552f56bef8fd9843f1b64997d589013ba3d2626e2623d73d5b6294847ac329",
                "42b7decef506ea5685ccbb6d5ec9a8b733db8c4ccb9899285cbf7afcf229d260",
                "4d47cb4a1c2e4050fa5194dbcf02d3d210ab142de6474ea9c1a507e8c73bb2ea",
                "2939d90fefc426730263e1d72751bff39482557974455aa3dd6fea938bfd2b91",
                "aff892095b5ad1694e10e25541475c88e4325c87ec649c625120d9dd6af0a4f6",
                "3441b72092a2c01b7d043e6e67f4a746222bb6dd7c06f687e2677d8acaa8c9f5",
                "be5bb66310d357d82f77c84f50fb9bd223401f68065e97727f9ca389631ae78a",
                "44fed838badef937705c89f759501bdc0d7b4a0b811573dde4a28cc6a2ffd285",
                "b65d815926f8b3a3bacd419a32a571170670c7f06c43b41d186534310bf57554",
                "511703f6d397079529911049daf4248d58cc0b2658e49fc0ea4d061290dbf372",
                "2b3aa7cdd535274ad11146bc9c7ed320fe976507756250269deb150fd8070889",
                "45f0c3c622569956e92e124a9abd7042d80e96d12d1830f0917c08f5dbb12fc6",
                "e0d74038b3446cac61b4e737129b3dd3321916282f392e185e5aaf84ee005d2b",
                "42e1f2582e0429d230e8955953645f47427d9c41663096fcde7b9e7d800a568a",
                "72f0e01bb29bbb72e02a8e609003c2ce2b6e4182fa906c05cf5b801dfa61bf62",
                "d73af8ecf2680dfe6a26cf2c5d0c044562f1a402e491bf8dc25d88e26d803494",
                "50d41a2b51a7e344cc7bae7a7b2ffa12a780884d7075dad826c50b862a7a24bf",
                "ff19790dfccc750c64d703b03ae5913b214c9b4e0d093f41000068d24f7f2953",
                "5a68bff628a2015e70b545f57cc3972eb03f5567d21b85d89e478dfff07600bc",
                "00a109b723fe6d0e3d3fa886daa22566f5365e018a297b4f17d0a77ccb7eb533",
                "0e2d7af103aeb1b5f605152eaed2cfa2b106882415652d535a6fe8b63112478d",
                "b7d9d401214903c354d79d67de6710e73fbbb914f6dbde376713306e327e489f",
                "a336155e5c31ab2f5f83e4a9bdd2d5a6c8e159e5916ae3462b829f57859fb41e",
                "6534d5bf0bb0da713a3d47617d51264aaa9eb0e91b5398451ad0a6e273576462",
                "f323de584d80cc9f34cc782387727e19412a28c563d79e42697a017114c734eb",
                "4255a9b7f00595e8602349c1e1beb3e5ef6166bc37158aa35cabd853ee4b3f91",
                "54565de8c4c3a59714422c0df1fb68dad33b8f0cfe6cecdc31700072f4f97fc3",
                "cce1e55bb9875fc01c429ca4399bb621abbf3e960a2665fa791b10e00a6bf234",
                "fb1767be8b13ec5d78e556c49cb74e9a95e88538d1e4474d8c914bd154ef25a1",
                "35454a0eb39aeff9d4267b1cdf74a5ef3c56d37e1c4f0c66b841bdaae1305359",
                "646252785992cca2df7e0f43b000b7330729e8b2ac475164cf637a17cd53904b",
                "40eb36bd578391b1632c44d3d3fd03ca5ef951d9783d76c527462ab2913623d2",
                "e0955ff672903114f0f10a4c0d6ee9494d8f86e3840641e7aa7f74d9bf9d8319",
                "72cc6ecb1160e2f9880c260ea09f7380dd8b085a327421cc82c2f7b9ae5a8697",
                "79f72f7349983b66879482983512aa0c77f073c06c272b1f4a01753053035b15",
                "87a537149f1224129af2ba22e045e9f350f450e139d743b3ae6bb8658155751a",
                "b7176881b2781ddb625881306b1ffb4c639413e94b46e1f98a7d79e4d17ddab1",
                "49deddbf97b2abe8136a1a5281d2ea590b207df3f585f3543adcde1b87a8e64e",
                "694df7380d22286a366d7de429976938a399e9463244366445c36ad045e6e955",
                "017d07b579bab2b6618325361c44751c4f840db1b9d8ef8a6aabf8f64f9d5df3",
                "c3cdc5f3aa929b6d33004f7a6a483c21480d023808a962e9cdf474318e078ba5",
                "413bcac16fe3d9c65764b32a84184fb49f26b13dc6c7de95eb4ede1f7c8a20d6",
                "53fa4eba45f8c40b624f8f20c1528506decb5e6eb544524380e4fdbd99323e83",
                "071549fcfdfabf1a712b8239a46ce15025c308c2edfeb768749ca72ecd75aaa0",
                "1fdeb99d857681e17fb04d8134765f46ba826e885645bc98f892892d6c3d9cdb",
                "be24e76d7e3c6d9d15ec52705e26e930f12aa6661b82c78b5ea07764c2b0a52d",
                "41fe23c39f288081f43b3e1af8a224138b5a9956b4527ad3140276a203a8ecb1",
                "63af837276a8ab8efd2515212fddc928795eabbeb9e3f412599d26a9ec8fbee2",
                "3bf77faa15ab6de3351db3357b0d6afd066873cce605012ff8a3166ecea92ccc",
                "435376bcaa19b8489e8023d1284eeeae6d3fef666671b82b90b952b57fca5c3a",
                "f51367d9f372a913ab80c5535e21106708e865b62f7dc9503344a20882dc555a",
                "3fa988463a817c011a5c2473adbf417db5379c8c9d6785718e61a08060b935f5",
                "c179eabada8608eb38dfb381752c8643f74540145ab2809882029d5aa04e4a61",
                "1042da59527ef0a75eacf98f193b780b08ad6a898832f98330705c028104902f",
                "40e13f881026901e8cff7d596204d806d2ed502cb8d81a4586b0d442f44921a3",
                "d8dd39e25d6e68cdaee5f649e69d1c5236b963d7dd2e16e870d0c72acef5b5b0",
                "a0807c9ab55ab21ef8430b65f0858e9770811786457af177adca8b1f77480494",
                "0d84e02bd5141a6fda84b68094af97076a97262474b53085131b8f0b99e239c1",
                "341064dcc5278aeb141ba742e89359de820e966b35f129d2958d92805a970a6b",
                "48a3900a93b95a300da71e890c4e1a1e837255cbbfccb8f2590e129419870aaf",
                "95cc664b58cdff76e462e46f63243bc8b3fc8e1c690edff55bb4eea61f7b0072",
                "bd30bad7260b389e81f345a053743d1594a4816da150576508bb01254d44b3c3",
                "544fb30b55cfd67bdafa0d04d68b67ecfabb44cec257cc67cde10bdf2a9f2bb2",
                "71785729d82149959abb93902691bd0d1cbaf3bfa21faeb5dac5e540d9d809a3",
                "e3da2c1650a362c73479ca3bbbc0590197b40045bf3534eb95bb1d79ff04fb03",
                "791dc2addc9d5634fd1690f16a040eddd1793d6b370e91118472e1c43372de3f",
                "2540eba0b00b656f4237a9119d7abdded69ac4c58c7cad412ab8f1577b889038",
                "e6ed308385b595f4e579bcbede4fd7fd57d574819e1abf1c1b7b2773009a15f0",
                "31bc86b68b39f546a16b05b2154d111c95f885fd866fec47712e382f38aeaa7d",
                "1db7e4f042c5e2b229c36be94eecc89c53a037515f551e1633dc51fb33348681",
                "f4a1409368d2178bc8f0eb151b6430705abc5245e786db831885ebc356431c2a",
                "f07c97255fbc21b4339daa373088f76524978ccfd02beac12f517ffcd7cece56",
                "36a268e980643da7a44a9bced8d5b16f3e23fcc275deb6e314a637ba0e789112",
                "99ca93e3d65071edd136f8a45437d6a10867d00dd54187dd2e4f7e1fcb1b1437",
            ],
            "bbefcacf2e8f7ffefacae8787ad88bad17aed59429d5306c429d6119b7612eb2",
            vec![
                "0a43663899d5e3697fbe30e0ed03bc94077651bf267316034791f9d74a880efd",
                "5c5a63ea59240410d291571f26b7b082c645e9935b572cacbb01f2598d9e650a",
                "6c335694ff8ed2a665f30afa954d2cca11d779fa31df32988eca82f4b863825e",
                "5eeaacb199069a7efae65e6e2660e6e4c40f034f3507c6307fa419bda2f89b68",
                "f4eec52aac2504b97a2a074fe7a30d35f8ea4a93eef335b665066d9badfc950a",
                "25ddb8af44e3222276a491ff1cbd6781c324bd59c3ccfc3f3168c96ffbed83ce",
                "2ae39e138cc695cceef4fd17bf3f1ef399064e2a45d70578a0ce21442030d5aa",
                "f1e467327b7434c610e552337a5b133dbafa4187c90a904ffce143cb3c3ef1c5",
                "f57b0ad0f1a34bb4e86accbec02a510ff3eeb01227ff6a9c470988c76ec6c3ee",
                "c16999fded3e396869e55dbf7744e9df1500bdb97e79eedc4b7d97f4448e20b6",
                "29490422e4f092c19700b22869a470f0d450cc6530cc4ab523395ee280879adb",
                "5fec2e461402697c00ac20f73a7397b434dc4535d2c0cc159f84a06899d32414",
            ],
            vec![
                "f04ae82df8a03cc594e4646898d411b673f0eabf69dc9d830e8afe6138777268",
                "8a30e46165c9353ab647722059897dc3135176bca89096e33534b3fcded80308",
                "098f162de64b61e490d92745802fe2912cb8aab4e943779a21aabdfe636af8a0",
                "899d2f32c1a24b1ec33dd88c20ba7851f37de393e63ba9a8332549c35e81607b",
                "88394a4873400f73aa146321f8e06ea22c419ef5c50c9c79d7dd58551dc6f1f2",
                "79d2e7dafb84cf13de53ec03ccd01c2bb86645e0a77354702a1709aac299f693",
                "204afec76428b95c5a9ca64a6143ad1a8e154490bbda86eef7d96ee86d023599",
                "f6850b310a07cb26f0a4d231f3984bf3148dcf74d29779a377295119b98a0569",
                "42afbbb145391faee34d2c00edd4f7144827b40f8ba061efb95f9c634aa2d05e",
                "759ee93542d144cc3a877acb86f573e480950e8e2d542d273c19a1f7944798ee",
                "e646ceb03aee4bad78a8fc67e40f083cf123ef204e09e75ab4b788cf444beb0e",
                "12a57a2e9429115481695da3b7cd8a465644f3b9a2d593d89d76a070dfc70f64",
                "1f334a63a5f218489a38765ee4f595f98bf28f83cdff98eefa2a82454034e903",
                "f6b38a7ae9bc413dab8c6b89f43fb637cda0ca87c36f83a2f4109e060067cb36",
                "13cc1d5ea371b9e5411b8d8948823ef12962dd73f0099e4b89d7ed0eeb2ffa9a",
                "3bb92654b9d121b73de2dded69894244cd6f51313176f5acd39e0a329e0b8025",
            ],
            vec![
                "0a43663899d5e3697fbe30e0ed03bc94077651bf267316034791f9d74a880efd",
                "5c5a63ea59240410d291571f26b7b082c645e9935b572cacbb01f2598d9e650a",
                "6c335694ff8ed2a665f30afa954d2cca11d779fa31df32988eca82f4b863825e",
                "5eeaacb199069a7efae65e6e2660e6e4c40f034f3507c6307fa419bda2f89b68",
                "f4eec52aac2504b97a2a074fe7a30d35f8ea4a93eef335b665066d9badfc950a",
                "25ddb8af44e3222276a491ff1cbd6781c324bd59c3ccfc3f3168c96ffbed83ce",
                "2ae39e138cc695cceef4fd17bf3f1ef399064e2a45d70578a0ce21442030d5aa",
                "f1e467327b7434c610e552337a5b133dbafa4187c90a904ffce143cb3c3ef1c5",
                "f57b0ad0f1a34bb4e86accbec02a510ff3eeb01227ff6a9c470988c76ec6c3ee",
                "c16999fded3e396869e55dbf7744e9df1500bdb97e79eedc4b7d97f4448e20b6",
                "29490422e4f092c19700b22869a470f0d450cc6530cc4ab523395ee280879adb",
                "5fec2e461402697c00ac20f73a7397b434dc4535d2c0cc159f84a06899d32414",
                "c27893d526d8e0a5766b6ecdd348ba7f7a440f8227e8cf4c03e4809216a61986",
                "4e661c4958802b526368a9b585fbd859df0afe937eb9d68572a5271cffd0eb9d",
                "43f206979504240d448acab20806e34e96962b8e5078776b08b11eaac94c4fa3",
                "3390a75a5e39a8e24889881ea55a8d191bac1e9698c2ced90e56d0795de96352",
                "14f9a7e853bd87a8e9e50b8c6bc4cfa32dde95d085e925821ff0d5f173953ad2",
                "3dff49f4683b05c1c7ef93bb35b5c897be742e74120d8a14b3a2929fb610b795",
                "f74d30cedb22d4f34029257d675a264a056a6805e1d657d024e21fe1b06b3c86",
                "c3437cc73bff03c3974006f896c11485e79f540550487668b358e659106b88fa",
                "6a6d6e24b1f16c20c2f601dcb1b15c3a18b48920d85d30e8e628a45f6fb84b9c",
                "ca81d80d9f59d9d52d4aae2424a5670ddb70ef9f0495e195b9f113d9d7f71aae",
                "506d4a9db799699f3d4bc97c29ba1b5d1e2016688ff8df891d70206cd08f62e4",
                "1190cfd76e6bc7afeefafa65d669ef8caf7b3e1dabcc44ba625a728276d016c9",
            ],
        )];

        for (keys, values, root, str_query_keys, sibling_hashes, deleted_keys) in test_data {
            let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
            let mut data = UpdateData { data: Cache::new() };
            for idx in 0..keys.len() {
                data.data.insert(
                    hex::decode(keys[idx]).unwrap(),
                    hex::decode(values[idx]).unwrap(),
                );
            }

            for key in deleted_keys {
                data.data
                    .entry(hex::decode(key).unwrap())
                    .and_modify(|x| *x = hex::decode(vec![]).unwrap())
                    .or_insert(hex::decode(vec![]).unwrap());
            }

            let mut db = smt_db::InMemorySmtDB::default();
            let result = tree.commit(&mut db, &data).unwrap();

            assert_eq!(**result.lock().unwrap(), hex::decode(root).unwrap());

            let query_keys = str_query_keys
                .iter()
                .map(|k| hex::decode(k).unwrap())
                .collect::<NestedVec>();
            let mut proof = tree.prove(&mut db, &query_keys).unwrap();
            assert_eq!(
                proof
                    .sibling_hashes
                    .iter()
                    .map(hex::encode)
                    .collect::<Vec<String>>(),
                sibling_hashes
            );
            assert!(SparseMerkleTree::verify(
                &query_keys,
                &proof,
                &result.lock().unwrap(),
                KeyLength(32),
            )
            .unwrap());
            let valid_query_proof = proof.queries[0].clone();

            // the length of the key is invalid, invalid the length of the key in query_keys
            proof.queries[0] = valid_query_proof.clone();
            let mut invalid_query_keys = query_keys.clone();
            invalid_query_keys[0] = query_keys[0][2..].to_vec();
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(
                    &proof,
                    &invalid_query_keys,
                    KeyLength(32)
                )
                .unwrap_err(),
                SMTError::InvalidInput(String::from("The length of the key is invalid",))
            );
            // the length of the key is invalid, invalid the length of the key in queries
            let mut invalid_query_with_proof = proof.queries[0].clone();
            invalid_query_with_proof.pair = Arc::new(KVPair::new(
                &proof.queries[0].pair.0[2..],
                proof.queries[0].value(),
            ));
            proof.queries[0] = invalid_query_with_proof;
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidInput(String::from("The length of the key is invalid",))
            );

            // invalid bitmap length with zero value at first element
            proof.queries[0] = valid_query_proof.clone();
            let mut invalid_bitmap = proof.queries[0].bitmap.to_vec();
            invalid_bitmap[0] = 0;
            proof.queries[0].bitmap = Arc::new(invalid_bitmap);
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidBitmapLen
            );
            // empty proof value when query key and proof key differs (invalid proof)
            proof.queries[0] = valid_query_proof.clone();
            proof.queries[0].pair = Arc::new(KVPair::new(
                &hex::decode(keys[1]).unwrap(),
                &hex::decode(vec![]).unwrap(),
            ));
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidInput(String::from(
                    "Proof for which query key and proof key differs, must have a non-empty value",
                ))
            );
            // mismatched binary_bitmap length with query_key_binary length
            proof.queries[0] = valid_query_proof.clone();
            proof.queries[0].bitmap = Arc::new(vec![30; 33]);
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidBitmapLen
            );
            // invalid bitmap length with common_prefix
            proof.queries[0].bitmap = Arc::new(vec![31; 2]);
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidBitmapLen
            );

            // duplicate query checks
            // 1. mismatched bitmap len
            proof.queries[0] = valid_query_proof;
            let valid_bitmap = proof.queries[3].bitmap.clone();
            proof.queries[3].bitmap = Arc::new(vec![30]);
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidInput(String::from("Mismatched values or bitmap",))
            );
            // 2. mismatched values
            proof.queries[3].bitmap = valid_bitmap;
            proof.queries[3].pair = Arc::new(KVPair::new(
                proof.queries[3].key(),
                &hex::decode(values[3]).unwrap(),
            ));
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidInput(String::from("Mismatched values or bitmap",))
            );
            // 3. mismatched keys
            proof.queries[3].pair = Arc::new(KVPair::new(
                &hex::decode(keys[4]).unwrap(),
                &hex::decode(values[3]).unwrap(),
            ));
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidBitmapLen
            );

            // mismatched length of keys and the queries
            proof.queries.clear();
            assert_eq!(
                SparseMerkleTree::verify_and_prepare_proof_map(&proof, &query_keys, KeyLength(32))
                    .unwrap_err(),
                SMTError::InvalidInput(String::from(
                    "Mismatched length of keys and the queries of the proof",
                ))
            );
        }
    }

    #[test]
    fn test_node_new_temp() {
        let node = Node::new_temp();
        assert_eq!(node.kind, NodeKind::Temp);
        assert_eq!(node.hash, KVPair(vec![], vec![]));
        assert_eq!(node.key, vec![]);
        assert_eq!(node.index, 0);
    }

    #[test]
    fn test_node_new_stub() {
        let node = Node::new_stub(&EMPTY_HASH);
        assert_eq!(node.kind, NodeKind::Stub);
        assert_eq!(
            node.hash,
            KVPair(
                vec![
                    1, 227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36,
                    39, 174, 65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85
                ],
                EMPTY_HASH.to_vec()
            )
        );
        assert_eq!(node.key, vec![]);
        assert_eq!(node.index, 0);
    }

    #[test]
    fn test_node_new_branch() {
        let node = Node::new_branch(&EMPTY_HASH, &EMPTY_HASH);
        assert_eq!(node.kind, NodeKind::Stub);
        assert_eq!(
            node.hash,
            KVPair(
                vec![
                    1, 227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36,
                    39, 174, 65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85, 227,
                    176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36, 39,
                    174, 65, 228, 100, 155, 147, 76, 164, 149, 153, 27, 120, 82, 184, 85
                ],
                vec![
                    75, 19, 46, 241, 48, 12, 27, 199, 145, 196, 180, 77, 220, 0, 183, 136, 246,
                    186, 252, 25, 209, 147, 94, 123, 7, 53, 181, 96, 62, 196, 225, 67
                ]
            )
        );
        assert_eq!(node.key, vec![]);
        assert_eq!(node.index, 0);
    }

    #[test]
    fn test_node_new_leaf() {
        let node = Node::new_leaf(&KVPair(
            vec![10, 11, 12, 13, 14, 15],
            vec![16, 17, 18, 19, 20],
        ));
        assert_eq!(node.kind, NodeKind::Leaf);
        assert_eq!(
            node.hash,
            KVPair(
                vec![0, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
                vec![
                    189, 48, 148, 168, 3, 139, 17, 26, 138, 59, 6, 102, 38, 115, 95, 225, 41, 141,
                    147, 173, 215, 231, 167, 69, 122, 198, 83, 105, 201, 165, 11, 128
                ]
            )
        );
        assert_eq!(node.key, vec![10, 11, 12, 13, 14, 15]);
        assert_eq!(node.index, 0);
    }

    #[test]
    fn test_node_new_empty() {
        let node = Node::new_empty();
        assert_eq!(node.kind, NodeKind::Empty);
        assert_eq!(node.hash, KVPair(vec![2], EMPTY_HASH.to_vec()));
        assert_eq!(node.key, vec![]);
        assert_eq!(node.index, 0);
    }
}
