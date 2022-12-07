/*
 * Copyright © 2022 Lisk Foundation
 *
 * See the LICENSE file at the top-level directory of this distribution
 * for licensing information.
 *
 * Unless otherwise agreed in a custom licensing agreement with the Lisk Foundation,
 * no part of this software, including this file, may be copied, modified,
 * propagated, or distributed except according to the terms contained in the
 * LICENSE file.
 *
 * Removal or modification of this copyright notice is prohibited.
 */
const os = require('os');
const path = require('path');
const fs = require('fs');
const { StateDB, SparseMerkleTree } = require('../main');
const fixtures = require('./fixtures/update_tree.json');
const SMTFixtures = require('./fixtures/smt_fixtures.json');
const JumboFixtures = require('./fixtures/smt_jumbo_fixtures.json');
const removeTreeFixtures = require('./fixtures/remove_tree.json');
const removeExtraTreeFixtures = require('./fixtures/remove_extra_tree.json');
const ProofFixtures = require('./fixtures/smt_proof_fixtures.json');

describe('SparseMerkleTree', () => {
    jest.setTimeout(100000);

    describe('updates', () => {
		for (const test of fixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}
				const rootHash = await smt.update(Buffer.alloc(0), kvpair);

				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}

		for (const test of SMTFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}
				const rootHash = await smt.update(Buffer.alloc(0), kvpair);

				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}

		for (const test of JumboFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0)});
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);
				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}

		for (const test of removeTreeFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0)});
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);
				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}

		for (const test of removeExtraTreeFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0)});
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);
				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}
	});

	describe('prove', () => {
		for (const test of ProofFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const queryKeys = test.input.queryKeys.map(keyHex => Buffer.from(keyHex, 'hex'));
				const outputMerkleRoot = test.output.merkleRoot;
				const outputProof = test.output.proof;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0)});
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);

				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);

				const proof = await smt.prove(rootHash, queryKeys);

				const siblingHashesString = [];
				for (const siblingHash of proof.siblingHashes) {
					siblingHashesString.push(siblingHash.toString('hex'));
				}

				const queriesString = [];
				for (const query of proof.queries) {
					queriesString.push({
						key: query.key.toString('hex'),
						value: query.value.toString('hex'),
						bitmap: query.bitmap.toString('hex'),
					});
				}

				expect(siblingHashesString).toEqual(outputProof.siblingHashes);
				expect(queriesString).toEqual(outputProof.queries);
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(true);
			});
		}
	});

	describe('prove - Jumbo fixtures', () => {
		for (const test of JumboFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const queryKeys = test.input.queryKeys.map(keyHex => Buffer.from(keyHex, 'hex'));
				const outputMerkleRoot = test.output.merkleRoot;
				const outputProof = test.output.proof;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex')});
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0)});
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);

				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);

				const proof = await smt.prove(rootHash, queryKeys);

				const siblingHashesString = [];
				for (const siblingHash of proof.siblingHashes) {
					siblingHashesString.push(siblingHash.toString('hex'));
				}

				const queriesString = [];
				for (const query of proof.queries) {
					queriesString.push({
						key: query.key.toString('hex'),
						value: query.value.toString('hex'),
						bitmap: query.bitmap.toString('hex'),
					});
				}

				expect(siblingHashesString).toEqual(outputProof.siblingHashes);
				expect(queriesString).toEqual(outputProof.queries);
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(true);

				// delete fist 25% of queries
				const deletingKVPair = [];
				const deleteQueryKeys = [];
				for (let i = 0; i < Math.floor(inputKeys.length / 4); i += 1) {
					deletingKVPair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.alloc(0)});
					deleteQueryKeys.push(Buffer.from(inputKeys[i], 'hex'));
				}
				const rootAfterDelete = await smt.update(rootHash, deletingKVPair);
				const deletedKeyProof = await smt.prove(rootAfterDelete, queryKeys);

				const isInclusion = (proofQuery, queryKey) =>
					queryKey.equals(proofQuery.key) && !proofQuery.value.equals(Buffer.alloc(0));
				await expect(smt.verify(rootAfterDelete, queryKeys, deletedKeyProof)).resolves.toEqual(true);
				for (let i = 0; i < queryKeys.length; i += 1) {
					const query = queryKeys[i];
					const proofQuery = deletedKeyProof.queries[i];

					if (inputKeys.find(k => Buffer.from(k, 'hex').equals(query)) !== undefined && [...deleteQueryKeys, ...deletedKeys.map(keyHex => Buffer.from(keyHex, 'hex'))].find(k => k.equals(query)) === undefined) {
						expect(isInclusion(proofQuery, query)).toEqual(true);
					} else {
						expect(isInclusion(proofQuery, query)).toEqual(false);
					}
				}
			});
		}
	});

	describe('calculateRoot', () => {
		for (const test of ProofFixtures.testCases) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const outputMerkleRoot = test.output.merkleRoot;
				const outputProof = test.output.proof;

				const proof = {
					siblingHashes: outputProof.siblingHashes.map(q => Buffer.from(q, 'hex')),
					queries: outputProof.queries.map(q => ({
						key: Buffer.from(q.key, 'hex'),
						value: Buffer.from(q.value, 'hex'),
						bitmap: Buffer.from(q.bitmap, 'hex'),
					}))
				};

				await expect(smt.calculateRoot(proof)).resolves.toEqual(Buffer.from(outputMerkleRoot, 'hex'));
			});
		}
	});
});
