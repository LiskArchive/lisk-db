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
'use strict';

const os = require('os');
const path = require('path');
const fs = require('fs');
const crypto = require('crypto');
const { StateDB, NotFoundError } = require('../main');
const { getRandomBytes } = require('./utils');

describe('statedb', () => {
    const initState = [
        {
            key: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 0]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 0]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([0, 1, 0, 0, 0, 0, 1]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([2, 0, 0, 0, 0, 0]),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from('0000000d800067656e657369735f323', 'hex'),
            value: getRandomBytes(),
        },
        {
            key: Buffer.from([0, 0, 0, 15, 0, 0]),
            value: Buffer.alloc(0),
        },
    ];

    let db;
    let root;

    beforeAll(async () => {
        const dbPath = path.join(os.tmpdir(), 'state', Date.now().toString());
        fs.mkdirSync(dbPath, { recursive: true });
        db = new StateDB(dbPath);
        const writer = db.newReadWriter();
        for (const pair of initState) {
            await writer.set(pair.key, pair.value);
        }
        root = await db.commit(writer, 0, Buffer.alloc(0));
    });

    afterAll(() => {
        db.close();
    });

    describe('StateDB', () => {
        it('should open DB', () => {
            expect(db).not.toBeUndefined();
        });

        it('should return false when called has if key does not exist', async () => {
            await expect(db.has(getRandomBytes())).resolves.toEqual(false);
        });

        it('should throw NotFoundError if data does not exist', async () => {
            await expect(db.get(getRandomBytes())).rejects.toThrow(NotFoundError);
        });

        it('should get the value if exist', async () => {
            await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
        });

        it('should return zero buffer when value length is zero', async () => {
            await expect(db.get(Buffer.from([0, 0, 0, 15, 0, 0]))).resolves.toEqual(Buffer.alloc(0));
        });

        it('should get the empty Buffer if exist but empty', async () => {
            const writer = db.newReadWriter();
            const key = getRandomBytes();
            await writer.set(key, Buffer.alloc(0));
            await expect(writer.get(key)).resolves.toEqual(Buffer.alloc(0));
        });

        it('should return true when called has if key exist', async () => {
            await expect(db.has(initState[0].key)).resolves.toEqual(true);
        });

        it('should iterate with specified range with limit', async () => {
            const stream = db.iterate({
                gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                limit: 2,
            });

            const values = await new Promise((resolve, reject) => {
                const result = [];
                stream
                    .on('data', kv => {
                        result.push(kv);
                    })
                    .on('err', err => {
                        reject(err);
                    })
                    .on('end', () => {
                        resolve(result);
                    });
            });

            expect(values).toEqual(initState.slice(1, 3));
        });

        it('should iterate with specified range with limit in reverse order', async () => {
            const stream = db.iterate({
                gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                limit: 2,
                reverse: true,
            });

            const values = await new Promise((resolve, reject) => {
                const result = [];
                stream
                    .on('data', kv => {
                        result.push(kv);
                    })
                    .on('err', err => {
                        reject(err);
                    })
                    .on('end', () => {
                        resolve(result);
                    });
            });

            expect(values).toEqual(initState.slice(2, 4).reverse());
        });

        it('should return empty if no data exist', async () => {
            const stream = db.iterate({
                gte: Buffer.from([2, 0, 0, 0, 0, 0, 0, 0, 1]),
                lte: Buffer.from([3, 0, 0, 0, 0, 1, 1, 0, 1]),
                limit: 2,
                reverse: true,
            });

            const values = await new Promise((resolve, reject) => {
                const result = [];
                stream
                    .on('data', kv => {
                        result.push(kv);
                    })
                    .on('err', err => {
                        reject(err);
                    })
                    .on('end', () => {
                        resolve(result);
                    });
            });

            expect(values).toEqual([]);
        });

        it('should get empty buffer multiple times', async () => {
            const writer = db.newReadWriter();
            const val = await writer.get(initState[7].key);
            expect(val).toEqual(Buffer.alloc(0));
        });

        describe('currentState', () => {
            it('if current state dose not exist, it should return emptyHash with zero version', async () => {
                const dbPath = path.join(os.tmpdir(), 'state', Date.now().toString());
                fs.mkdirSync(dbPath, { recursive: true });
                const temp_db = new StateDB(dbPath);
                const res = await temp_db.getCurrentState();
                expect(res.version).toEqual(0);
                const hasher = crypto.createHash('sha256');
                hasher.update(Buffer.alloc(0));
                const emptyHash = hasher.digest();
                expect(res.root).toEqual(emptyHash);
            });

            it('should return initiate values at the beginning', async () => {
                const res = await db.getCurrentState();
                expect(res.version).toEqual(0);
                expect(res.root).toEqual(root);
            });

            it('should not update state and root if readonly is specified', async () => {
                const writer = db.newReadWriter();
                await writer.set(initState[0].key, getRandomBytes());
                await db.commit(writer, 0, Buffer.alloc(0), { readonly: true });
                const afterCommit = await db.getCurrentState();
                expect(afterCommit.version).toEqual(0);
                expect(afterCommit.root).toEqual(root);
            });

            it('should not update if commit is rejected', async () => {
                const writer = db.newReadWriter();
                await writer.set(initState[0].key, getRandomBytes());
                await expect(db.commit(writer, 1, root, { readonly: true, checkRoot: true, expectedRoot: getRandomBytes() }))
                    .rejects.toThrow('Invalid state root `Not matching with expected`');
                const currentState = await db.getCurrentState();
                expect(currentState.version).toEqual(0);
                expect(currentState.root).toEqual(root);
            });

            it('check commit and then revert', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[0].key, newValue);

                const nextRoot = await db.commit(writer, 1, root);
                const afterCommit = await db.getCurrentState();
                expect(afterCommit.version).toEqual(1);
                expect(afterCommit.root).toEqual(nextRoot);
                await expect(db.get(initState[0].key)).resolves.toEqual(newValue);

                await db.revert(nextRoot, 1);
                const afterRevert = await db.getCurrentState();
                expect(afterRevert.version).toEqual(0);
                expect(afterRevert.root).toEqual(root);
            });
        });

        describe('should not have same values for a snapshot and main db after commit or revert', () => {
            it('check commit and then revert also check reader and readWriter after those functions', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[0].key, newValue);

                const nextRoot = await db.commit(writer, 1, root);
                await expect(writer.get(initState[0].key)).resolves.toEqual(newValue);
                const afterCommit = await db.getCurrentState();
                expect(afterCommit.version).toEqual(1);
                const reader = db.newReader();
                await expect(reader.get(initState[0].key)).resolves.toEqual(newValue);
                expect(afterCommit.root).toEqual(nextRoot);
                await expect(db.get(initState[0].key)).resolves.toEqual(newValue);

                await db.revert(nextRoot, 1);
                const afterRevert = await db.getCurrentState();
                expect(afterRevert.version).toEqual(0);
                // old reader and old writer should have the newValue
                await expect(reader.get(initState[0].key)).resolves.toEqual(newValue);
                await expect(writer.get(initState[0].key)).resolves.toEqual(newValue);
                await expect(writer.has(initState[0].key)).resolves.toEqual(true);
                await expect(writer.has(getRandomBytes())).resolves.toEqual(false);
                expect(afterRevert.root).toEqual(root);
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
                // newReader should have the old value
                const newReader = db.newReader();
                await expect(newReader.get(initState[0].key)).resolves.toEqual(initState[0].value);
                // newWriter should have the old value
                const newWriter = db.newReadWriter();
                await expect(newWriter.get(initState[0].key)).resolves.toEqual(initState[0].value);
            });
        });

        describe('commit', () => {
            it('should not update state if readonly is specified', async () => {
                const writer = db.newReadWriter();
                await writer.set(initState[0].key, getRandomBytes());
                const nextRoot = await db.commit(writer, 0, Buffer.alloc(0), { readonly: true });

                expect(nextRoot).not.toEqual(root);
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
            });

            it('should reject if checkRoot is true and different from expected', async () => {
                const writer = db.newReadWriter();
                await writer.set(initState[0].key, getRandomBytes());
                await expect(db.commit(writer, 1, root, { readonly: true, checkRoot: true, expectedRoot: getRandomBytes() }))
                    .rejects.toThrow('Invalid state root `Not matching with expected`');
            });
        });

        describe('revert', () => {
            it('should remove the created state', async () => {
                const writer = db.newReadWriter();
                const newKey = getRandomBytes();
                await writer.set(newKey, getRandomBytes());

                const nextRoot = await db.commit(writer, 1, root);
                await expect(db.has(newKey)).resolves.toEqual(true);

                const original = await db.revert(nextRoot, 1);
                await expect(db.has(newKey)).resolves.toEqual(false);
                expect(original).toEqual(root);
            });

            it('should re-create the deleted state', async () => {
                const writer = db.newReadWriter();
                await writer.del(initState[0].key);

                const nextRoot = await db.commit(writer, 1, root);
                await expect(db.has(initState[0].key)).resolves.toEqual(false);

                const original = await db.revert(nextRoot, 1);
                await expect(db.has(initState[0].key)).resolves.toEqual(true);
                expect(original).toEqual(root);
            });

            it('should revert the updated state to the original state', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[0].key, newValue);

                const nextRoot = await db.commit(writer, 1, root);
                await expect(db.get(initState[0].key)).resolves.toEqual(newValue);

                const original = await db.revert(nextRoot, 1);
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
                expect(original).toEqual(root);
            });
        });

        describe('finalize', () => {
            it('should remove all diff except the height specified', async () => {
                for (let i = 0; i < 10; i += 1) {
                    const writer = db.newReadWriter();
                    await writer.set(getRandomBytes(), getRandomBytes());
                    root = await db.commit(writer, i + 2, root);
                    // current state should be updated after each commit
                    const afterCommit = await db.getCurrentState();
                    expect(afterCommit.version).toEqual(i + 2);
                    expect(afterCommit.root).toEqual(root);
                }

                await expect(db.finalize(11)).resolves.toBeUndefined();
                root = await db.revert(root, 11);
                await expect(db.revert(root, 10)).rejects.toThrow('Diff not found for height: `10`');
                // current state should be updated after revert
                const after_revert = await db.getCurrentState();
                expect(after_revert.version).toEqual(10);
                expect(after_revert.root).toEqual(root);
            });
        });

        describe('StateReadWriter', () => {
            it('should return values with range', async () => {
                const writer = db.newReadWriter();
                await writer.set(Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 3]), getRandomBytes());

                const result = await writer.range({
                    gte: Buffer.from('00000002800000000000', 'hex'),
                    lte: Buffer.from('000000028000ffffffff', 'hex'),
                });

                expect(result).toHaveLength(0);
            });

            it('should return updated value with range', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);

                const result = await writer.range({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                expect(result).toHaveLength(2);
                expect(result[0]).toEqual({
                    key: initState[1].key,
                    value: newValue,
                });
                expect(result[1]).toEqual(initState[2]);
            });

            it('should return updated value with range in reverse', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);

                const result = await writer.range({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                    reverse: true,
                });

                expect(result).toHaveLength(2);
                expect(result[0].value).toEqual(initState[3].value);
                expect(result[1].value).toEqual(initState[2].value);
            });

            it('should return to original value after restoreSnapshot', async () => {
                const writer = db.newReadWriter();
                const index = writer.snapshot();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);
                writer.restoreSnapshot(index);

                const result = await writer.range({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                expect(result).toHaveLength(2);
                expect(result[0].value).toEqual(initState[1].value);
                expect(result[1].value).toEqual(initState[2].value);
            });

            it('should return to original value after restoreSnapshot with multiple snapshot', async () => {
                const writer = db.newReadWriter();
                const index = writer.snapshot();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);
                writer.snapshot()
                await writer.set(initState[1].key, getRandomBytes());
                writer.restoreSnapshot(index);

                const result = await writer.range({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                expect(result).toHaveLength(2);
                expect(result[0].value).toEqual(initState[1].value);
                expect(result[1].value).toEqual(initState[2].value);
            });

            it('should throw error with non existing snapshot', async () => {
                const writer = db.newReadWriter();
                writer.snapshot();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);
                writer.snapshot()
                await writer.set(initState[1].key, getRandomBytes());
                expect(() => writer.restoreSnapshot(99)).toThrow('Invalid usage');
            });

            it('should throw an error when the writer is closed', async () => {
                const writer = db.newReadWriter();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);
                await expect(writer.get(initState[1].key)).resolves.toEqual(newValue);
                writer.close();
                expect(() => writer.get(initState[1].key)).rejects.toThrow();
            });
        });

        describe('StateReader', () => {
            const nonExistingKey = Buffer.from([255, 255]);

            it('should not have set', () => {
                const reader = db.newReader();
                expect(reader.set).toBeUndefined();
            });

            it('should not have del', () => {
                const reader = db.newReader();
                expect(reader.del).toBeUndefined();
            });

            it('should fail if DB does not have key', async () => {
                const reader = db.newReader();
                await expect(reader.get(nonExistingKey)).rejects.toThrow('does not exist');
            });

            it('should return zero buffer when value length is zero', async () => {
                const reader = db.newReader();
                await expect(reader.get(Buffer.from([0, 0, 0, 15, 0, 0]))).resolves.toEqual(Buffer.alloc(0));
            });

            it('should return associated value', async () => {
                const reader = db.newReader();
                await expect(reader.get(initState[0].key)).resolves.toEqual(initState[0].value);
            });

            it('should return true when called has if key exist', async () => {
                const reader = db.newReader();
                await expect(reader.has(initState[0].key)).resolves.toEqual(true);
            });

            it('should iterate with specified range with limit', async () => {
                const reader = db.newReader();
                const stream = reader.iterate({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                const values = await new Promise((resolve, reject) => {
                    const result = [];
                    stream
                        .on('data', kv => {
                            result.push(kv);
                        })
                        .on('err', err => {
                            reject(err);
                        })
                        .on('end', () => {
                            resolve(result);
                        });
                });

                expect(values).toEqual(initState.slice(1, 3));
            });

            it('should create read stream with specified range with limit', async () => {
                const reader = db.newReader();
                const stream = reader.createReadStream({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                const values = await new Promise((resolve, reject) => {
                    const result = [];
                    stream
                        .on('data', kv => {
                            result.push(kv);
                        })
                        .on('err', err => {
                            reject(err);
                        })
                        .on('end', () => {
                            resolve(result);
                        });
                });

                expect(values).toEqual(initState.slice(1, 3));
            });

            it('should throw an error when the reader is closed', async () => {
                const reader = db.newReader();
                await expect(reader.get(initState[0].key)).resolves.toEqual(initState[0].value);
                reader.close();
                expect(() => reader.get(initState[1].key)).rejects.toThrow();
            });
        });

        describe('checkpoint', () => {
            let tmpPath;
            beforeEach(() => {
                tmpPath = fs.mkdtempSync("");
            });

            it('should create checkpoint', async () => {
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
                await expect(db.get(initState[1].key)).resolves.toEqual(initState[1].value);
                await expect(db.get(initState[2].key)).resolves.toEqual(initState[2].value);

                await db.checkpoint(tmpPath + '/test_db');

                db = new StateDB(tmpPath + '/test_db');
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
                await expect(db.get(initState[1].key)).resolves.toEqual(initState[1].value);
                await expect(db.get(initState[2].key)).resolves.toEqual(initState[2].value);
            });

            it('should failed to create checkpoint because directory is not empty', async () => {
                await expect(db.get(initState[0].key)).resolves.toEqual(initState[0].value);
                await expect(db.get(initState[1].key)).resolves.toEqual(initState[1].value);
                await expect(db.get(initState[2].key)).resolves.toEqual(initState[2].value);

                await expect(db.checkpoint(tmpPath)).rejects.toThrow();
            });
        });

        describe('proof', () => {
            it('should generate non-inclusion proof and verify that a result is correct', async () => {
                const queries = [getRandomBytes(38), getRandomBytes(38)];
                const proof = await db.prove(root, queries);

                const result = await db.verify(root, queries, proof);

                expect(result).toEqual(true);
            });

            it('should generate wrong non-inclusion proof and verify that a result is not correct', async () => {
                const queries = [getRandomBytes(38), getRandomBytes(38)];
                const proof = await db.prove(root, queries);

                // change sibling hash in proof to make it wrong
                proof.siblingHashes[0] = getRandomBytes(32);

                const result = await db.verify(root, queries, proof);

                expect(result).toEqual(false);
            });

            it('should generate inclusion proof and verify that a result is correct', async () => {
                const queries = [
                    Buffer.concat([initState[0].key.slice(0, 6), crypto.createHash('sha256').update(initState[0].key.slice(6)).digest()]),
                    Buffer.concat([initState[1].key.slice(0, 6), crypto.createHash('sha256').update(initState[1].key.slice(6)).digest()]),
                    Buffer.concat([initState[2].key.slice(0, 6), crypto.createHash('sha256').update(initState[2].key.slice(6)).digest()])
                ];
                const proof = await db.prove(root, queries);

                const result = await db.verify(root, queries, proof);

                expect(result).toEqual(true);
            });

            it('should generate wrong inclusion proof and verify that a result is not correct', async () => {
                const queries = [
                    Buffer.concat([initState[0].key.slice(0, 6), crypto.createHash('sha256').update(initState[0].key.slice(6)).digest()]),
                    Buffer.concat([initState[1].key.slice(0, 6), crypto.createHash('sha256').update(initState[1].key.slice(6)).digest()]),
                    Buffer.concat([initState[2].key.slice(0, 6), crypto.createHash('sha256').update(initState[2].key.slice(6)).digest()])
                ];
                const proof = await db.prove(root, queries);

                // change sibling hash in proof to make it wrong
                proof.siblingHashes[0] = getRandomBytes(32);

                const result = await db.verify(root, queries, proof);

                expect(result).toEqual(false);
            });
        });

        describe('calculateRoot', () => {
            it('should calculate sparse merkle tree root', async () => {
                const queries = [getRandomBytes(38), getRandomBytes(38)];
                const proof = await db.prove(root, queries);

                await expect(db.calculateRoot(proof)).resolves.toEqual(root);
            });

            it('should calculate wrong sparse merkle tree root', async () => {
                const queries = [getRandomBytes(38), getRandomBytes(38)];
                const proof = await db.prove(root, queries);

                // change sibling hash in proof to make it wrong
                proof.siblingHashes[0] = getRandomBytes(32);

                await expect(db.calculateRoot(proof)).resolves.not.toEqual(root);
            });
        });
    });
});
