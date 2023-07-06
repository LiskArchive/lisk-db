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
const { SparseMerkleTree } = require('../main');
const { getRandomBytes } = require('./utils');
const { isInclusionProofForQueryKey } = require('../utils');

const FixturesInclusionProof = require('./fixtures/fixtures_no_delete_inclusion_proof.json');
const FixturesNonInclusionProof = require('./fixtures/fixtures_delete_non_inclusion_proof.json');

describe('SparseMerkleTree', () => {
	jest.setTimeout(100000);

	describe('updates and deletes', () => {
		for (const test of [...FixturesInclusionProof.testCases, ...FixturesNonInclusionProof.testCases]) {
			// eslint-disable-next-line no-loop-func
			it(test.description, async () => {
				const smt = new SparseMerkleTree(32);
				const inputKeys = test.input.keys;
				const inputValues = test.input.values;
				const deletedKeys = test.input.deleteKeys;
				const outputMerkleRoot = test.output.merkleRoot;

				const kvpair = [];
				for (let i = 0; i < inputKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex') });
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0) });
				}

				const rootHash = await smt.update(Buffer.alloc(0), kvpair);
				expect(rootHash.toString('hex')).toEqual(outputMerkleRoot);
			});
		}
	});

	describe('prove', () => {
		for (const test of [...FixturesInclusionProof.testCases, ...FixturesNonInclusionProof.testCases]) {
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
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex') });
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0) });
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

                /*
                * We do not know which testcase is inclusion or non-inclusion so define the two
                * following functions to call correct verification function based on the result
                * of these functions.
                */
                const isNonInclusionProof = (queriesKeys, proofQueries) => {
                    for (let i = 0; i < queriesKeys.length; i++) {
                        if (isInclusionProofForQueryKey(queriesKeys[i], proofQueries[i])) {
                            return false;
                        }
                    }

                    return true;
                }
                const isInclusionProof = (queriesKeys, proofQueries) => {
                    for (let i = 0; i < queriesKeys.length; i++) {
                        if (!isInclusionProofForQueryKey(queriesKeys[i], proofQueries[i])) {
                            return false;
                        }
                    }

                    return true;
                }
                if (isNonInclusionProof(queryKeys, proof.queries)) {
                    await expect(smt.verifyNonInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(true);
                    await expect(smt.verifyInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(false);
                } else if (isInclusionProof(queryKeys, proof.queries)) {
                    await expect(smt.verifyInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(true);
                    await expect(smt.verifyNonInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(false);
                }

				expect(siblingHashesString).toEqual(outputProof.siblingHashes);
				expect(queriesString).toEqual(outputProof.queries);
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, proof)).resolves.toEqual(true);

				// modified bitmap should fail
				const zeroPrependedProof = {
					...proof,
					queries: proof.queries.map(q => ({
						...q,
						bitmap: Buffer.concat([Buffer.from([0]), q.bitmap])
					})),
				};
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, zeroPrependedProof)).resolves.toEqual(false);
				const zeroAppendedProof = {
					...proof,
					queries: proof.queries.map(q => ({
						...q,
						bitmap: Buffer.concat([q.bitmap, Buffer.from([0])])
					})),
				};
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, zeroAppendedProof)).resolves.toEqual(false);

				// modified sibling hashes should fail
				const randomSiblingPrependedProof = { ...proof };
				randomSiblingPrependedProof.siblingHashes = [getRandomBytes(), ...randomSiblingPrependedProof.siblingHashes];
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, randomSiblingPrependedProof)).resolves.toEqual(false);
                await expect(smt.verifyInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, randomSiblingPrependedProof)).resolves.toEqual(false);
				await expect(smt.verifyNonInclusionProof(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, randomSiblingPrependedProof)).resolves.toEqual(false);

				const randomSiblingAppendedProof = { ...proof };
				randomSiblingAppendedProof.siblingHashes = [...randomSiblingAppendedProof.siblingHashes, getRandomBytes()];
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, randomSiblingAppendedProof)).resolves.toEqual(false);

				// removed random query should fail
				const randomQueryRemovedProof = { ...proof };
				randomQueryRemovedProof.queries.splice(Math.floor(Math.random() * randomQueryRemovedProof.queries.length), 1);
				await expect(smt.verify(Buffer.from(outputMerkleRoot, 'hex'), queryKeys, randomQueryRemovedProof)).resolves.toEqual(false);
			});
		}
	});

	describe('prove - check inclusion and non inclusion', () => {
		for (const test of [...FixturesInclusionProof.testCases, ...FixturesNonInclusionProof.testCases]) {
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
					kvpair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.from(inputValues[i], 'hex') });
				}

				for (let i = 0; i < deletedKeys.length; i += 1) {
					kvpair.push({ key: Buffer.from(deletedKeys[i], 'hex'), value: Buffer.alloc(0) });
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
					deletingKVPair.push({ key: Buffer.from(inputKeys[i], 'hex'), value: Buffer.alloc(0) });
					deleteQueryKeys.push(Buffer.from(inputKeys[i], 'hex'));
				}
				const rootAfterDelete = await smt.update(rootHash, deletingKVPair);
				const deletedKeyProof = await smt.prove(rootAfterDelete, queryKeys);

				await expect(smt.verify(rootAfterDelete, queryKeys, deletedKeyProof)).resolves.toEqual(true);
				for (let i = 0; i < queryKeys.length; i += 1) {
					const query = queryKeys[i];
					const proofQuery = deletedKeyProof.queries[i];

					if (inputKeys.find(k => Buffer.from(k, 'hex').equals(query)) !== undefined && [...deleteQueryKeys, ...deletedKeys.map(keyHex => Buffer.from(keyHex, 'hex'))].find(k => k.equals(query)) === undefined) {
						expect(isInclusionProofForQueryKey(query, proofQuery)).toEqual(true);
					} else {
						expect(isInclusionProofForQueryKey(query, proofQuery)).toEqual(false);
					}
				}
			});
		}
	});

	describe('calculateRoot', () => {
		for (const test of [...FixturesInclusionProof.testCases, ...FixturesNonInclusionProof.testCases]) {
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

		describe('when invalid sibling hash is provided', () => {
			it('should resolve to error', async () => {
				const smt = new SparseMerkleTree(32);

				const proof = {
					siblingHashes: [],
					queries: [{
						key: Buffer.from([1, 2, 3]),
						value: Buffer.from([1, 2, 3]),
						bitmap: Buffer.from([1]),
					}],
				};

				await expect(smt.calculateRoot(proof)).rejects.toThrow('Invalid input');
			});
		});

		describe('when there is too many sibling hashes', () => {
			for (const test of [...FixturesInclusionProof.testCases, ...FixturesNonInclusionProof.testCases]) {
				// eslint-disable-next-line no-loop-func
				it('should resolve to error', async () => {
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

					// add unused sibling hash
					proof.siblingHashes.push(getRandomBytes(32));

					await expect(smt.calculateRoot(proof)).rejects.toThrow('Not all sibling hashes were used');
				});
			}
		});
	});
});
