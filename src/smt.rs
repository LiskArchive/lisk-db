use sha2::{Digest, Sha256};
use std::cmp;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use thiserror::Error;

use crate::types::{
    Cache, Height, KVPair, KeyLength, NestedVec, SharedKVPair, SharedNestedVec, SharedVec,
    StructurePosition, SubtreeHeight, VecOption, DB,
};
use crate::utils;

const PREFIX_INT_LEAF_HASH: u8 = 0;
const PREFIX_INT_BRANCH_HASH: u8 = 1;
const PREFIX_INT_EMPTY: u8 = 2;
const HASH_SIZE: usize = 32;
const PREFIX_SIZE: usize = 6;
static PREFIX_LEAF_HASH: &[u8] = &[0];
static PREFIX_BRANCH_HASH: &[u8] = &[1];
static PREFIX_EMPTY: &[u8] = &[2];

type Hasher = fn(node_hashes: &[Arc<Vec<u8>>], structure: &[u8], height: Height) -> Arc<Vec<u8>>;
type SharedNode = Arc<Mutex<Node>>;

trait SortDescending {
    fn sort_descending(&mut self);
}

#[derive(Error, Debug)]
pub enum SMTError {
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
    Stub,
    Temp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateData {
    data: Cache,
}

#[derive(Clone, Debug)]
pub struct Proof {
    pub sibling_hashes: NestedVec,
    pub queries: Vec<QueryProof>,
}

#[derive(Clone, Debug)]
pub struct QueryProof {
    pub pair: Arc<KVPair>,
    pub bitmap: Arc<Vec<u8>>,
}

#[derive(Clone, Debug)]
struct QueryProofWithProof {
    query_proof: QueryProof,
    binary_bitmap: Vec<bool>,
    ancestor_hashes: NestedVec,
    sibling_hashes: NestedVec,
    hash: Vec<u8>,
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

struct QueryHashesExtraInfo {
    height: Height,
    target_id: usize,
    max_index: usize,
}

struct GenerateResultData<'a> {
    query_key: &'a [u8],
    current_node: &'a Node,
    ref_mut_vecs: QueryHashesRefMut<'a>,
    height: Height,
    query_height: Height,
}

struct QueryHashesRefMut<'a> {
    ancestor_hashes: &'a mut VecDeque<Vec<u8>>,
    sibling_hashes: &'a mut VecDeque<Vec<u8>>,
    binary_bitmap: &'a mut Vec<bool>,
}

struct QueryHashesInfo<'a> {
    layer_nodes: Vec<SharedNode>,
    layer_structure: Vec<u8>,
    ref_mut_vecs: QueryHashesRefMut<'a>,
    extra: QueryHashesExtraInfo,
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

pub struct SparseMerkleTree {
    root: SharedVec,
    key_length: KeyLength,
    subtree_height: SubtreeHeight,
    max_number_of_nodes: usize,
    hasher: Hasher,
}

fn key_hash(key: &[u8]) -> Vec<u8> {
    let prefix = &key[..PREFIX_SIZE];
    let body = &key[PREFIX_SIZE..];
    let mut hasher = Sha256::new();
    hasher.update(body);
    let result = hasher.finalize();
    [prefix, result.as_slice()].concat()
}

fn value_hash(value: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(value);
    let result = hasher.finalize();
    result.to_vec()
}

fn leaf_hash(pair: &KVPair) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(PREFIX_LEAF_HASH);
    hasher.update(pair.key());
    hasher.update(pair.value());
    let result = hasher.finalize();
    result.to_vec()
}

fn branch_hash(node_hash: &[u8]) -> Vec<u8> {
    let mut hasher = Sha256::new();
    hasher.update(PREFIX_BRANCH_HASH);
    hasher.update(node_hash);
    let result = hasher.finalize();
    result.to_vec()
}

fn empty_hash() -> Vec<u8> {
    let hasher = Sha256::new();
    let result = hasher.finalize();
    result.to_vec()
}

fn tree_hasher(node_hashes: &[Arc<Vec<u8>>], structure: &[u8], height: Height) -> Arc<Vec<u8>> {
    let mut node_hashes: Vec<Arc<Vec<u8>>> = node_hashes.to_vec();
    let mut structure: Vec<u8> = structure.to_vec();
    let mut height = height;

    while node_hashes.len() != 1 {
        let mut next_hashes = vec![];
        let mut next_structure = vec![];
        let mut i = 0;

        while i < node_hashes.len() {
            if structure[i] == height.into() {
                let branch = [
                    (*node_hashes[i]).as_slice(),
                    (*node_hashes[i + 1]).as_slice(),
                ]
                .concat();
                let hash = branch_hash(&branch);
                next_hashes.push(Arc::new(hash));
                next_structure.push(structure[i] - 1);
                i += 1;
            } else {
                next_hashes.push(Arc::clone(&node_hashes[i]));
                next_structure.push(structure[i]);
            }
            i += 1;
        }

        if height.is_equal_to(1) {
            return Arc::clone(&next_hashes[0]);
        }

        height = height.sub(1);
        node_hashes = next_hashes;
        structure = next_structure;
    }

    Arc::clone(&node_hashes[0])
}

fn parent_node(
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
    hasher: Hasher,
) -> Result<SubTree, SMTError> {
    let mut layer_nodes = layer_nodes.to_vec();
    let mut layer_structure = layer_structure.to_vec();
    let mut height = height;

    while !height.is_equal_to(0) {
        let mut next_layer_nodes: Vec<SharedNode> = vec![];
        let mut next_layer_structure: Vec<u8> = vec![];
        let mut i = 0;
        while i < layer_nodes.len() {
            if layer_structure[i] != height.into() {
                next_layer_nodes.push(Arc::clone(&layer_nodes[i]));
                next_layer_structure.push(layer_structure[i]);
                i += 1;
                continue;
            }

            let parent = parent_node(&layer_nodes, &layer_structure, tree_map, i)?;
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
                return SubTree::from_data(&structure, &nodes, hasher);
            }
            return SubTree::from_data(&[0], &next_layer_nodes, hasher);
        }
        layer_nodes = next_layer_nodes;
        layer_structure = next_layer_structure;
        height = height.sub(1);
    }

    SubTree::from_data(&[0], &layer_nodes, hasher)
}

fn calc_next_info(info: &mut QueryHashesInfo, next_info: &mut NextQueryHashesInfo, i: usize) {
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
            calc_next_info(&mut info, &mut next_info, i);
            i += 2;
        }
        let new_extra = QueryHashesExtraInfo::new(
            info.extra.height.sub(1),
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
    if !utils::arr_eq_bool(&q.binary_path(), &original.binary_path()) {
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
        self.data.insert(key_hash(&key), value_hash(&value));
    }
    /// Called with a key that was `delete`d from the batch.
    fn delete(&mut self, key: Box<[u8]>) {
        self.data.insert(key_hash(&key), vec![]);
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

    #[allow(dead_code)]
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

    pub fn new_with_hash(data: Cache) -> Self {
        let mut new_data = Cache::new();
        for (k, v) in data {
            if !v.is_empty() {
                new_data.insert(key_hash(&k), value_hash(&v));
            } else {
                new_data.insert(key_hash(&k), vec![]);
            }
        }
        Self { data: new_data }
    }

    pub fn entries(&self) -> (SharedNestedVec, SharedNestedVec) {
        let mut kv_pair = vec![];
        for (k, v) in self.data.iter() {
            kv_pair.push(SharedKVPair(k, v));
        }
        kv_pair.sort_by(|a, b| a.0.cmp(b.0));
        let mut keys = vec![];
        let mut values = vec![];
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
            empty_hash()
        } else {
            leaf_hash(&pair)
        };
        Self {
            query_proof: QueryProof::new_with_binary_bitmap(pair, binary_bitmap),
            binary_bitmap: binary_bitmap.to_vec(),
            ancestor_hashes: ancestor_hashes.to_vec(),
            sibling_hashes: sibling_hashes.to_vec(),
            hash: hashed_key,
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
        self.query_proof.bitmap = Arc::new(utils::bools_to_bytes(&self.binary_bitmap));
    }

    fn binary_path(&self) -> Vec<bool> {
        utils::bytes_to_bools(self.query_proof.key())[..self.height()].to_vec()
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
        let data = [PREFIX_BRANCH_HASH, node_hash].concat();
        Self {
            kind: NodeKind::Stub,
            hash: KVPair::new(&data, node_hash),
            key: vec![],
            index: 0,
        }
    }

    fn new_branch(left_hash: &[u8], right_hash: &[u8]) -> Self {
        let combined = [left_hash, right_hash].concat();
        let data = [PREFIX_BRANCH_HASH, &combined].concat();
        let hashed = branch_hash(&combined);
        Self {
            kind: NodeKind::Stub,
            hash: KVPair::new(&data, &hashed),
            key: vec![],
            index: 0,
        }
    }

    fn new_leaf(pair: &KVPair) -> Self {
        let h = leaf_hash(pair);
        let data = [PREFIX_LEAF_HASH, pair.key(), pair.value()].concat();
        Self {
            kind: NodeKind::Leaf,
            hash: KVPair::new(&data, &h),
            key: pair.key_as_vec(),
            index: 0,
        }
    }

    fn new_empty() -> Self {
        let h = empty_hash();
        let data = [PREFIX_EMPTY].concat();
        Self {
            kind: NodeKind::Empty,
            hash: KVPair::new(&data, &h),
            key: vec![],
            index: 0,
        }
    }
}

impl SubTree {
    pub fn new(data: &[u8], key_length: KeyLength, hasher: Hasher) -> Result<Self, SMTError> {
        if data.is_empty() {
            return Err(SMTError::InvalidInput(String::from("keys length is zero")));
        }
        let node_length: usize = data[0] as usize + 1;
        let structure = &data[1..node_length + 1];
        let node_data = &data[node_length + 1..];
        let mut nodes: Vec<SharedNode> = vec![];
        let mut idx = 0;

        let key_length: usize = key_length.into();
        while idx < node_data.len() {
            match node_data[idx] {
                PREFIX_INT_LEAF_HASH => {
                    let kv = KVPair::new(
                        &node_data[idx + PREFIX_LEAF_HASH.len()
                            ..idx + PREFIX_LEAF_HASH.len() + key_length],
                        &node_data[idx + PREFIX_LEAF_HASH.len() + key_length
                            ..idx + PREFIX_LEAF_HASH.len() + key_length + HASH_SIZE],
                    );
                    let node = Node::new_leaf(&kv);
                    nodes.push(Arc::new(Mutex::new(node)));
                    idx += PREFIX_LEAF_HASH.len() + key_length + HASH_SIZE;
                },
                PREFIX_INT_BRANCH_HASH => {
                    let node_hash = &node_data[idx + PREFIX_BRANCH_HASH.len()
                        ..idx + PREFIX_BRANCH_HASH.len() + HASH_SIZE];
                    nodes.push(Arc::new(Mutex::new(Node::new_stub(node_hash))));
                    idx += PREFIX_BRANCH_HASH.len() + HASH_SIZE;
                },
                PREFIX_INT_EMPTY => {
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

        SubTree::from_data(structure, &nodes, hasher)
    }

    pub fn from_data(
        structure: &[u8],
        nodes: &[SharedNode],
        hasher: Hasher,
    ) -> Result<Self, SMTError> {
        let height = structure
            .iter()
            .max()
            .ok_or_else(|| SMTError::Unknown(String::from("Invalid structure")))?;

        let node_hashes = nodes
            .iter()
            .map(|n| Arc::new(n.lock().unwrap().hash.value_as_vec()))
            .collect::<Vec<Arc<Vec<u8>>>>();
        let calculated = hasher(&node_hashes, structure, height.into());

        Ok(Self {
            structure: structure.to_vec(),
            nodes: nodes.to_vec(),
            root: calculated,
        })
    }

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

    pub fn encode(&self) -> Vec<u8> {
        let node_length = (self.structure.len() - 1) as u8;
        let node_hashes: Vec<Vec<u8>> = self
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
        vecs: QueryHashesRefMut<'a>,
        extra: QueryHashesExtraInfo,
    ) -> Self {
        QueryHashesInfo {
            layer_nodes,
            layer_structure,
            ref_mut_vecs: QueryHashesRefMut {
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
    fn proof_queries(&self, query_with_proofs: &[QueryProofWithProof]) -> Vec<QueryProof> {
        let mut proof_queries = vec![];
        for query in query_with_proofs {
            proof_queries.push(QueryProof {
                pair: Arc::clone(&query.query_proof.pair),
                bitmap: Arc::clone(&query.query_proof.bitmap),
            });
        }

        proof_queries
    }

    fn sibling_data(
        &mut self,
        db: &mut impl DB,
        queries: &[Vec<u8>],
    ) -> Result<(Vec<QueryProofWithProof>, NestedVec), SMTError> {
        let mut query_with_proofs: Vec<QueryProofWithProof> = vec![];
        let mut root = self.get_subtree(db, &self.root.lock().unwrap())?;
        let mut ancestor_hashes = vec![];
        for query in queries {
            let query_proof = self.generate_query_proof(db, &mut root, query, Height(0))?;
            query_with_proofs.push(query_proof.clone());
            ancestor_hashes.extend(query_proof.ancestor_hashes);
        }

        Ok((query_with_proofs, ancestor_hashes))
    }

    fn filter_map(proof: &Proof) -> HashMap<Vec<bool>, QueryProofWithProof> {
        let mut filter_map: HashMap<Vec<bool>, QueryProofWithProof> = HashMap::new();
        for query in &proof.queries {
            let binary_bitmap = utils::strip_left_false(&utils::bytes_to_bools(&query.bitmap));
            let binary_path = utils::bytes_to_bools(&query.pair.0)[..binary_bitmap.len()].to_vec();

            filter_map.insert(
                binary_path,
                QueryProofWithProof::new_with_pair(
                    Arc::clone(&query.pair),
                    &binary_bitmap,
                    &[],
                    &[],
                ),
            );
        }

        filter_map
    }

    fn verify_keys(proof: &Proof, query_keys: &[Vec<u8>], key_length: KeyLength) -> bool {
        for (i, key) in query_keys.iter().enumerate() {
            if key.len() != key_length.into() {
                return false;
            }
            let query = &proof.queries[i];
            if utils::is_bytes_equal(key, query.key()) {
                continue;
            }
            let key_binary = utils::bytes_to_bools(key);
            let query_key_binary = utils::bytes_to_bools(query.key());
            let common_prefix = utils::common_prefix(&key_binary, &query_key_binary);
            let binary_bitmap = utils::strip_left_false(&utils::bytes_to_bools(&query.bitmap));
            if binary_bitmap.len() > common_prefix.len() {
                return false;
            }
        }

        true
    }

    fn get_subtree(&self, db: &impl DB, node_hash: &[u8]) -> Result<SubTree, SMTError> {
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

        SubTree::new(&value, self.key_length, self.hasher)
    }

    fn calc_bins<'a>(
        &mut self,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        height: Height,
    ) -> Result<Bins<'a>, SMTError> {
        let mut keys = vec![];
        let mut values = vec![];

        for _ in 0..self.max_number_of_nodes {
            keys.push(vec![]);
            values.push(vec![]);
        }
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

    fn calc_updated_info<'a>(
        &mut self,
        db: &mut impl DB,
        current_subtree: &SubTree,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        height: Height,
    ) -> Result<UpdatedInfo, SMTError> {
        let bins = self.calc_bins(key_bin, value_bin, height)?;
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

    fn update_subtree<'a>(
        &mut self,
        db: &mut impl DB,
        key_bin: &'a [&'a [u8]],
        value_bin: &'a [&'a [u8]],
        current_subtree: &SubTree,
        height: Height,
    ) -> Result<SubTree, SMTError> {
        if key_bin.is_empty() {
            return Ok(current_subtree.clone());
        }
        let updated = self.calc_updated_info(db, current_subtree, key_bin, value_bin, height)?;
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
            self.hasher,
        )?;
        let value = new_subtree.encode();
        db.set(&KVPair::new(&new_subtree.root, &value))
            .map_err(|err| SMTError::Unknown(err.to_string()))?;

        Ok(new_subtree)
    }

    fn update_one_node(
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
        db: &mut impl DB,
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
            NodeKind::Leaf => {
                SubTree::from_data(&[0], &[Arc::clone(&info.current_node)], self.hasher)?
            },
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

    fn left_right_nodes(
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

    fn update_node(
        &mut self,
        db: &mut impl DB,
        info: UpdateNodeInfo,
    ) -> Result<(Vec<SharedNode>, Vec<u8>), SMTError> {
        let total_data = info.length_bins[info.length_bins.len() - 1] - info.length_base;
        if total_data == 0 {
            return Ok((
                vec![Arc::clone(&info.current_node)],
                vec![info.structure_pos.into()],
            ));
        }
        if total_data == 1 {
            if let Some((node, structure)) = self.update_one_node(&info)? {
                return Ok((vec![node], vec![structure.into()]));
            }
        }

        if info.structure_pos == self.subtree_height.into() {
            let (node, structure) = self.update_same_height(db, &info)?;
            return Ok((vec![node], vec![structure.into()]));
        }

        let (left_node, right_node) = self.left_right_nodes(&info)?;
        let idx = info.key_bins.len() / 2;
        let left_info = UpdateNodeInfo::new(
            &info.key_bins[0..idx],
            &info.value_bins[0..idx],
            &info.length_bins[0..idx],
            left_node,
            info.length_base,
            info.height,
            info.structure_pos.add(1),
        );
        let (mut left_nodes, mut left_heights) = self.update_node(db, left_info)?;
        let right_info = UpdateNodeInfo::new(
            &info.key_bins[idx..],
            &info.value_bins[idx..],
            &info.length_bins[idx..],
            right_node,
            info.length_bins[idx - 1],
            info.height,
            info.structure_pos.add(1),
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
        let mut bin_offset = 0;
        let mut current_node: Option<SharedNode> = None;
        let bin_idx = self.find_index(query_key, height)?;
        let mut h = 0;
        for i in 0..current_subtree.nodes.len() {
            h = current_subtree.structure[i];
            current_node = Some(Arc::clone(&current_subtree.nodes[i]));
            let new_offset = 1 << (self.subtree_height.sub_to_usize(h));
            if bin_offset <= bin_idx && bin_idx < bin_offset + new_offset {
                break;
            }
            bin_offset += new_offset
        }

        Ok((Arc::clone(&current_node.unwrap()), h.into()))
    }

    fn calc_query_proof_from_result(
        &mut self,
        db: &mut impl DB,
        d: &GenerateResultData,
    ) -> Result<QueryProofWithProof, SMTError> {
        let mut ancestor_hashes = d.ref_mut_vecs.ancestor_hashes.clone();
        let sibling_hashes = Vec::from(d.ref_mut_vecs.sibling_hashes.clone());
        let binary_bitmap = d.ref_mut_vecs.binary_bitmap.clone();
        if d.current_node.kind == NodeKind::Empty {
            let pair = Arc::new(KVPair::new(d.query_key, &[]));
            return Ok(QueryProofWithProof::new_with_pair(
                pair,
                d.ref_mut_vecs.binary_bitmap,
                &Vec::from(ancestor_hashes),
                &(sibling_hashes),
            ));
        }

        if d.current_node.kind == NodeKind::Leaf {
            ancestor_hashes.push_back(d.current_node.hash.value_as_vec());
            let pair = Arc::new(KVPair::new(
                &d.current_node.key,
                &d.current_node.hash.key()[PREFIX_LEAF_HASH.len() + HASH_SIZE..],
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

    fn calc_query_hashes_extra_info(
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

    fn generate_query_proof(
        &mut self,
        db: &mut impl DB,
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
        let extra =
            self.calc_query_hashes_extra_info(current_subtree, &current_node.lock().unwrap())?;
        let info = QueryHashesInfo::new(
            current_subtree.nodes.clone(),
            current_subtree.structure.clone(),
            QueryHashesRefMut {
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
            ref_mut_vecs: QueryHashesRefMut {
                ancestor_hashes: &mut ancestor_hashes,
                sibling_hashes: &mut sibling_hashes,
                binary_bitmap: &mut binary_bitmap,
            },
            height,
            query_height,
        };
        self.calc_query_proof_from_result(db, &data)
    }

    fn calculate_root(sibling_hashes: &[Vec<u8>], queries: &mut [QueryProofWithProof]) -> Vec<u8> {
        queries.sort_descending();

        let mut sorted_queries = VecDeque::from(queries.to_vec());
        let mut next_sibling_hash = 0;

        while !sorted_queries.is_empty() {
            let query = &sorted_queries.pop_front().unwrap();
            if query.is_zero_height() {
                return query.hash.clone();
            }

            let mut sibling_hash: VecOption = None;

            if !sorted_queries.is_empty() && query.is_sibling_of(&sorted_queries[0]) {
                let sibling = sorted_queries.pop_front().unwrap();
                sibling_hash = Some(sibling.hash);
            } else if !query.binary_bitmap[0] {
                sibling_hash = Some(empty_hash());
            } else if query.binary_bitmap[0] {
                sibling_hash = Some(sibling_hashes[next_sibling_hash].clone());
                next_sibling_hash += 1;
            }
            let d = query.binary_key()[query.height() - 1];
            let mut next_query = query.clone();
            if !d {
                next_query.hash = branch_hash(
                    &[query.hash.as_slice(), sibling_hash.unwrap().as_slice()].concat(),
                );
            } else {
                next_query.hash = branch_hash(
                    &[sibling_hash.unwrap().as_slice(), query.hash.as_slice()].concat(),
                );
            }
            next_query.slice_bitmap();
            insert_and_filter_queries(next_query, &mut sorted_queries);
        }

        vec![]
    }

    pub fn new(root: &[u8], key_length: KeyLength, subtree_height: SubtreeHeight) -> Self {
        let max_number_of_nodes = 1 << subtree_height.u16();
        let r = if root.is_empty() {
            utils::empty_hash()
        } else {
            root.to_vec()
        };
        Self {
            root: Arc::new(Mutex::new(Arc::new(r))),
            key_length,
            hasher: tree_hasher,
            subtree_height,
            max_number_of_nodes,
        }
    }

    pub fn commit(
        &mut self,
        db: &mut impl DB,
        data: &mut UpdateData,
    ) -> Result<SharedVec, SMTError> {
        if data.is_empty() {
            return Ok(Arc::clone(&self.root));
        }
        let (update_keys, update_values) = data.entries();
        let root = self.get_subtree(db, &self.root.lock().unwrap())?;
        let new_root = self.update_subtree(db, &update_keys, &update_values, &root, Height(0))?;
        self.root = Arc::new(Mutex::new(new_root.root));
        Ok(Arc::clone(&self.root))
    }

    pub fn prove(&mut self, db: &mut impl DB, queries: &[Vec<u8>]) -> Result<Proof, SMTError> {
        if queries.is_empty() {
            return Ok(Proof {
                queries: vec![],
                sibling_hashes: vec![],
            });
        }
        let (mut query_with_proofs, ancestor_hashes) = self.sibling_data(db, queries)?;
        let proof_queries = self.proof_queries(&query_with_proofs);

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

    pub fn verify(
        query_keys: &[Vec<u8>],
        proof: &Proof,
        root: &[u8],
        key_length: KeyLength,
    ) -> Result<bool, SMTError> {
        if query_keys.len() != proof.queries.len() {
            return Ok(false);
        }

        if !Self::verify_keys(proof, query_keys, key_length) {
            return Ok(false);
        }
        let filter_map = Self::filter_map(proof);
        let mut filtered_proof = filter_map
            .values()
            .cloned()
            .collect::<Vec<QueryProofWithProof>>();

        Ok(utils::is_bytes_equal(
            root,
            &SparseMerkleTree::calculate_root(&proof.sibling_hashes, &mut filtered_proof),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::smt_db;

    #[test]
    fn test_subtree() {
        let test_data = vec![
            ("05030302020303001f930f4f669738b026406a872c24db29238731868957ae1de0e5a68bb0cf7da633e508533a13da9c33fc64eb78b18bd0646c82d6316697dece0aee5a3a92e45700082e6af17a61852d01dfc18e859c20b0b974472bf6169295c36ce1380c2550e16c16babfe7d3204f61852d100f553276ad154921988de3797622091f0581884b008b647996849b70889d2a382d8fa2f42405c3bca57189de0be52c92bbc03f0cd21194ddd776cf387a81d0117b6288e6a724ec14a58cdde3c196292191da360da800ec66ad4b484153de040869f8833a30a8fcde4fdf8fcbd78d33c2fb2182dd8ffa3b311d3a72a9aec8560c56c68d665ad54c5644d40ea4fc7ed914d4eea5da3c0400e93bd78ce150412056a9076cf58977ff1a697b1932abdd52d7b978fce69186d3a9cb7274eceac6b0807ce4db0763dc596cd00e59177172de6b5dd1593b33a78500c8c4673053da259999cbc9502aef75c3c0b84bce42b1d1a2d437df88d32b737bd36e7a6410939ac431914de947353f06bbbfc31c86609ec291ed9e13b665f86a", "7a208dc2a21cb829e5fa4dc7d876bef8e52ddd23ae5ea24c2567b264bcd91a23", vec![3, 3, 2, 2, 3, 3]),
            ("02010202020049720db77a5ca853713493d4e11926b417af0cae746a305a52f555738eed47cad58c7809f5cf4119cc0f25c224f7124d15b5d62ba93bc3d948db32871026f068018dfe7dfa8fb4a5a268168638c8cce0e26f87a227320aee691f8872ed6a3aba0e", "c0fcf4b2571622905dde0884ef56d494ad3481d28fa167466f970f2c633e2925", vec![1,2,2]),
            ("0f0404040404040404040404040404040401bbacc7102a28f2eecd0e4de3c130064e653d0118b1dc4129095901f190e70034019dcb747007aca526d4b0782ed20a88a5d48a4ab6276378bada201ab5b6e4d75b01e89b7270dd0ad80207e11422bfc28f8cda8932d59b1082486fa1bf5626ea0aba01858c61150861b89516244e07cfd9d3ebcb12b2d44c2de4e7e2faed96717202eb01f9437e84b231d85f7fc2690ed54b09e85c2e0fc98b26430f10418065374e40bf0189ae2184c9a2e70656ce37c89c903b258198ad6e9db66f135780f66d8613a6fd01058c3bef2957b130622e752f0a81ee8dcf60b4685675eb88e39d5150c954fe220161543e80c5356f580f8e7e4548576486ee754ffe22f4dd122ef48e41bffc7adc01f55a1089a16835a4cbe8b5e12227575ecfd99cd951e34b409f9b2ace6f25a49701e5dfbf3ecaf909728248a751e1a75f3b626777094fe1aab03ae6f526ddac799a01f88ad8cd4aec6cc4f8d2c2bc4a5f368fc9b877685eb55673baa01d652fa4c82b0182f8fb577797274de4f48d8bd7cc5a77068ea3c60477e8552b38c926466eba1101c149d0c79bc1355d763d01690139fd187a84488d534e7e38e4772279c3826b9b01006afab486675b0e3f9b6b06283da947df6749269fb8621afe843d5df942bce7011ead1b569f80edffa2044bf9d8b8703b970ca741b821127d6da69da83b52294f01c1a9d57b050c3ba96aca78a26c5eebc76bb51acab78ce70ed3bdea1ca9143cd8", "5a2f1f740cbea0944d5182fe8ef9190d7a07e8601d0b9fc1137d48b94ce73407", vec![4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4]),
        ];

        for (data, hash, structure) in test_data {
            let decoded_data = hex::decode(data).unwrap();
            let tree = SubTree::new(&decoded_data, KeyLength(32), tree_hasher).unwrap();
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
            let tree = SubTree::new(&decoded_data, KeyLength(32), tree_hasher).unwrap();
            assert_eq!(tree.encode(), decoded_data);
        }
    }

    #[test]
    fn test_empty_tree() {
        let mut tree = SparseMerkleTree::new(&[], KeyLength(32), Default::default());
        let mut data = UpdateData { data: Cache::new() };
        let mut db = smt_db::InMemorySmtDB::new();
        let result = tree.commit(&mut db, &mut data);

        assert_eq!(
            **result.unwrap().lock().unwrap(),
            hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .unwrap()
        );
    }

    #[test]
    fn test_small_tree_0() {
        let test_data = vec![(
            vec!["6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"],
            vec!["1406e05881e299367766d313e26c05564ec91bf721d31726bd6e46e60689539a"],
            "ccd1c136c75ffd2e3947466ad17dd6687d890ce50cbeb7ca7a4da638df482b96",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data);

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
            "6d13bfad2a210dc084b9a896f79243d58c7fbd2721181b86cdaed00af349f429",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data);

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
            "d336d7a29ec55728822a2f9ec6aae3bee549e743d50469d7fe924914348ff758",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data);

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
            "3f91f1b7bc96933102dcce6a6c9200c68146a8327c16b91f8e4b37f40e2e2fb4",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data);

            assert_eq!(
                **result.unwrap().lock().unwrap(),
                hex::decode(root).unwrap()
            );
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
                    "3f91f1b7bc96933102dcce6a6c9200c68146a8327c16b91f8e4b37f40e2e2fb4",
                    vec!["6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"],
                    vec![
                        "8d1ffa7d6c798b22e899eb01e2ff37aa38ca5d155c7787ec77e6818cb6058d50",
                        "b40ca4f4d1cc50ab10ec89fcde1a6587003f964c83f040f54e591c9dfc8a549f",
                        "9a428d826c80eeabda0ee5f77458d492b333a171ee781543e09ee62100786142",
                        "f6d10a31f5362e0ceada0b2fccabc648dc25d635bb3331f5cb0f499591f104b8",
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
                    "dd6e59d920b0a911ad43bbb6c97453128deece03f5799c13eae050cad6958368",
                    vec![
                        "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                        "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                        "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                    ],
                    vec![
                        "947f6a2a1faab9e68a23701e1eae120d401148983326794b05063e118f2a0513",
                        "f5a9a1f4a90cf891b6cd0ca1f82bbe90e88835c3aed9106805464890f2734fa1",
                        "c4288c46c16467a3c12928f99be89f9401b813ae123289a09f2ae1fa2211067a",
                        "eb57d25f6308dc90d5d2c34462153040443ef5241441a9e644957b0e51c33944",
                        "c81bf0eea251a7e8ee8bb2960d5e3c5847110496ed68ef889f4050e02f27cb8f",
                        "8d1ffa7d6c798b22e899eb01e2ff37aa38ca5d155c7787ec77e6818cb6058d50",
                        "17c59ac41e2255a5da6f06be5d677618084b6c253c20698cbafe8982fc320e29",
                        "d71d377dedbcfa43ec16423d839914e43edc96c9d2322caf020f51b01727579f",
                        "0debba7a991a1281f9d3e00f788c0325271119fa3839075e9b5fabb42837916e",
                        "fdaad6b0fe38314f5f2b74c745e1de1058ee0e9e1c42a8b025a655a6b1d1a1d3",
                        "94eb9f8005ae50cfae31cef5a25dd2b5d21c70c6f6d45483589cfe771a7ab597",
                        "8e3cad238038e7888db0286ba540cb349abcca266dd5403c6280b6c45cc61002",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data).unwrap();

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
            "5fc84e67172cd3d03bef61c4a726dc2889418afeaf1afc15a9f02caad777423a",
            vec![
                "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d",
                "4bf5122f344554c53bde2ebb8cd2b7e3d1600ad631c385a5d7cce23c7785459a",
                "dbc1b4c900ffe48d575b5da5c638040125f65db0fe3e24494b76ea986457d986",
                "084fed08b978af4d7d196a7446a86b58009e636b611db16211b65a9aadff29c5",
                "e52d9c508c502347344d8c07ad91cbd6068afc75ff6292f062a09ca381c89e71",
                "e77b9a9ae9e30b0dbdb6f510a264ef9de781501d7b6b92ae89eb059c5ab743db",
            ],
            vec![
                "72ec6f47671dc68bd661a55b0db40776638d504cd7768cfa1ab684a5a2b58df0",
                "6f6beacd3e8b18a49390762296f8c0c755b4c93b5960c7818719b0993ed20a53",
                "947f6a2a1faab9e68a23701e1eae120d401148983326794b05063e118f2a0513",
                "f5a9a1f4a90cf891b6cd0ca1f82bbe90e88835c3aed9106805464890f2734fa1",
                "9fabc5ebbc9b48138a45b8776b77699ce816fc6defa671d6485c735406267191",
                "eb57d25f6308dc90d5d2c34462153040443ef5241441a9e644957b0e51c33944",
                "f706374e4ba44ea13b8bbe6615f3715c785fd54e261e7ab4d9b32e9195c5f989",
                "c81bf0eea251a7e8ee8bb2960d5e3c5847110496ed68ef889f4050e02f27cb8f",
                "8d1ffa7d6c798b22e899eb01e2ff37aa38ca5d155c7787ec77e6818cb6058d50",
                "7ca70841e154f8f3876f667ade1bbe7dc26d03813432501291473e9ef03a3371",
                "b55661052dd2e2f7aea3f82f73ce7534ee4196ee8ab7ecab69cb029a4bdba203",
                "58193f723127ede64b63e37de54d5ef15df5379a7d71a562caf06edae231bfb7",
                "17c59ac41e2255a5da6f06be5d677618084b6c253c20698cbafe8982fc320e29",
                "d71d377dedbcfa43ec16423d839914e43edc96c9d2322caf020f51b01727579f",
                "fbf8316ffafee18eafa1e8113ea2f38f564c844ba475181460603edfe714a753",
                "44f3f97e5667f7b878e1c790ef3a8e5770296b21cd3e168dd65adf9c2fdc5a3b",
                "74395f5d785bd88fdb6720f9b8429f83501beea5a92582037450244b243e577d",
                "056c8542f08b631258fb6d545de8a9cab8bea8230ea6b26c3bb78c7cb7044737",
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
            let mut db = smt_db::InMemorySmtDB::new();
            let result = tree.commit(&mut db, &mut data).unwrap();

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
}
