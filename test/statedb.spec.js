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
                }

                await expect(db.finalize(11)).resolves.toBeUndefined();
                root = await db.revert(root, 11);
                await expect(db.revert(root, 10)).rejects.toThrow('Diff not found for height: `10`');
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
                expect(result[0].value).toEqual(newValue);
                expect(result[1].value).toEqual(initState[2].value);
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
                writer.snapshot();
                const newValue = getRandomBytes();
                await writer.set(initState[1].key, newValue);
                writer.restoreSnapshot();

                const result = await writer.range({
                    gte: Buffer.from([0, 0, 0, 0, 0, 0, 0, 0, 1]),
                    lte: Buffer.from([0, 0, 0, 0, 0, 1, 1, 0, 1]),
                    limit: 2,
                });

                expect(result).toHaveLength(2);
                expect(result[0].value).toEqual(initState[1].value);
                expect(result[1].value).toEqual(initState[2].value);
            });
        });

        describe('StateReader', () => {
            it('should not have set', () => {
                const reader = db.newReader();
                expect(reader.set).toBeUndefined();
            });

            it('should not have del', () => {
                const reader = db.newReader();
                expect(reader.del).toBeUndefined();
            });
        });
    });
});