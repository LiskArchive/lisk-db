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
// const ProofFixtures = require('./fixtures/smt_proof_fixtures.json');

describe('SparseMerkleTree', () => {
    jest.setTimeout(10000);

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

	// describe('generateMultiProof', () => {
	// 	let db: Database;
	// 	let smt: SparseMerkleTree;

	// 	beforeEach(() => {
	// 		db = new InMemoryDB();
	// 		smt = new SparseMerkleTree({ db, keyLength: 32 });
	// 	});

	// 	for (const test of ProofFixtures.testCases) {
	// 		// eslint-disable-next-line no-loop-func
	// 		it(test.description, async () => {
	// 			const inputKeys = test.input.keys;
	// 			const inputValues = test.input.values;
	// 			const queryKeys = test.input.queryKeys.map(keyHex => Buffer.from(keyHex, 'hex'));
	// 			const outputMerkleRoot = test.output.merkleRoot;
	// 			const outputProof = test.output.proof;

	// 			for (let i = 0; i < inputKeys.length; i += 1) {
	// 				await smt.update(Buffer.from(inputKeys[i], 'hex'), Buffer.from(inputValues[i], 'hex'));
	// 			}

	// 			const proof = await smt.generateMultiProof(queryKeys);

	// 			const siblingHashesString = [];
	// 			for (const siblingHash of proof.siblingHashes) {
	// 				siblingHashesString.push(siblingHash.toString('hex'));
	// 			}

	// 			const queriesString = [];
	// 			for (const query of proof.queries) {
	// 				queriesString.push({
	// 					key: query.key.toString('hex'),
	// 					value: query.value.toString('hex'),
	// 					bitmap: query.bitmap.toString('hex'),
	// 				});
	// 			}

	// 			expect(siblingHashesString).toEqual(outputProof.siblingHashes);
	// 			expect(queriesString).toEqual(outputProof.queries);
	// 			expect(verify(queryKeys, proof, Buffer.from(outputMerkleRoot, 'hex'), 32)).toBeTrue();
	// 		});
	// 	}
	// });

	// // TODO: Enable or migrate with new testing strategy. This test takes 20min~
	// // eslint-disable-next-line jest/no-disabled-tests
	// describe.skip('generateMultiProof - Jumbo fixtures', () => {
	// 	let smt;
    // beforeAll(() => {
    //     const dbPath = path.join(os.tmpdir(), Date.now().toString());
    //     fs.mkdirSync(dbPath, { recursive: true });
    //     smt = new StateDB(dbPath, { keyLength: 32 });
    // });
    // afterAll(async () => {
    //     await smt.close();
    // });

	// 	for (const test of JumboFixtures.testCases) {
	// 		// eslint-disable-next-line no-loop-func
	// 		it(test.description, async () => {
	// 			const inputKeys = test.input.keys;
	// 			const inputValues = test.input.values;
	// 			const removeKeys = test.input.deleteKeys;
	// 			const queryKeys = test.input.queryKeys.map(keyHex => Buffer.from(keyHex, 'hex'));
	// 			const outputMerkleRoot = test.output.merkleRoot;
	// 			const outputProof = test.output.proof;

	// 			for (let i = 0; i < inputKeys.length; i += 1) {
	// 				await smt.update(Buffer.from(inputKeys[i], 'hex'), Buffer.from(inputValues[i], 'hex'));
	// 			}

	// 			for (const key of removeKeys) {
	// 				await smt.remove(Buffer.from(key, 'hex'));
	// 			}

	// 			const proof = await smt.generateMultiProof(queryKeys);

	// 			const siblingHashesString = [];
	// 			for (const siblingHash of proof.siblingHashes) {
	// 				siblingHashesString.push(siblingHash.toString('hex'));
	// 			}

	// 			const queriesString = [];
	// 			for (const query of proof.queries) {
	// 				queriesString.push({
	// 					key: query.key.toString('hex'),
	// 					value: query.value.toString('hex'),
	// 					bitmap: query.bitmap.toString('hex'),
	// 				});
	// 			}

	// 			expect(siblingHashesString).toEqual(outputProof.siblingHashes);
	// 			expect(queriesString).toEqual(outputProof.queries);
	// 			expect(verify(queryKeys, proof, Buffer.from(outputMerkleRoot, 'hex'), 32)).toBeTrue();
	// 		});
	// 	}
	// });
});
