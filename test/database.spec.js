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
const { Database, Batch, NotFoundError, InMemoryDatabase } = require('../main');
const { getRandomBytes } = require('./utils');

describe('database', () => {
    describe('Database', () => {
        let db;
        beforeAll(() => {
            const dbPath = path.join(os.tmpdir(), 'db', Date.now().toString());
            fs.mkdirSync(dbPath, { recursive: true });
            db = new Database(dbPath);
        });

        afterAll(() => {
            db.close();
        });

        it('should be able to open after closing', async () => {
            const newDBPath = path.join(os.tmpdir(), 'db', Date.now().toString());
            fs.mkdirSync(newDBPath, { recursive: true });
            const newDB = new Database(newDBPath);
            const key = getRandomBytes();
            const value = getRandomBytes();
            await newDB.set(key, value);
            newDB.close();

            const reopenDB = new Database(newDBPath);
            await expect(reopenDB.get(key)).resolves.toEqual(value);
        });

        it('should open DB', () => {
            expect(db).not.toBeUndefined();
        });

        it('should return false when called has if key does not exist', async () => {
            await expect(db.has(getRandomBytes())).resolves.toEqual(false);
        });

        it('should return true when called has if key exist', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.has(kv.key)).resolves.toEqual(true);
        });

        it('should throw NotFoundError when data does not exist', async () => {
            await expect(db.get(getRandomBytes())).rejects.toThrow(NotFoundError);
        });

        it('should get the value if exist', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.get(kv.key)).resolves.toEqual(kv.value);
        });

        it('should delete value', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.has(kv.key)).resolves.toEqual(true);

            const anotherBatch = new Batch();
            anotherBatch.del(kv.key);
            await db.write(anotherBatch);
            await expect(db.has(kv.key)).resolves.toEqual(false);

        });

        it('should clear all value', async () => {
            const pairs = [
                { key: getRandomBytes(), value: getRandomBytes() },
                { key: getRandomBytes(), value: getRandomBytes() },
            ];
            const batch = new Batch();
            for (const kv of pairs) {
                batch.set(kv.key, kv.value);
            }
            await db.write(batch);

            await expect(db.has(pairs[0].key)).resolves.toEqual(true);
            await expect(db.has(pairs[1].key)).resolves.toEqual(true);

            await db.clear();

            await expect(db.has(pairs[0].key)).resolves.toEqual(false);
            await expect(db.has(pairs[1].key)).resolves.toEqual(false);
        });

        it('should clear partial value', async () => {
            const pairs = [
                { key: getRandomBytes(), value: getRandomBytes() },
                { key: getRandomBytes(), value: getRandomBytes() },
            ];
            const batch = new Batch();
            for (const kv of pairs) {
                batch.set(kv.key, kv.value);
            }
            await db.write(batch);

            await expect(db.has(pairs[0].key)).resolves.toEqual(true);
            await expect(db.has(pairs[1].key)).resolves.toEqual(true);

            await db.clear({
                gte: pairs[0].key,
                lte: pairs[0].key,
                limit: 1,
            });

            await expect(db.has(pairs[0].key)).resolves.toEqual(false);
            await expect(db.has(pairs[1].key)).resolves.toEqual(true);
        });

        describe('iteration', () => {
            let pairs;
            beforeAll(async () => {
                await db.clear();
                pairs = [
                    {
                        key: Buffer.from([0, 0, 0]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([0, 0, 1]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([1, 0, 0]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([1, 0, 1]),
                        value: getRandomBytes(),
                    },
                ];
                const batch = new Batch();
                for (const pair of pairs) {
                    batch.set(pair.key, pair.value);
                }
                await db.write(batch);
            });

            it('should iterate with specified range with limit', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([0, 0, 1]),
                    lte: Buffer.from([1, 0, 1]),
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

                expect(values).toEqual(pairs.slice(1, 3));
            });

            it('should iterate with specified range with limit in reverse order', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([0, 0, 1]),
                    lte: Buffer.from([1, 0, 1]),
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

                expect(values).toEqual(pairs.slice(2, 4).reverse());
            });

            it('should return empty if no data exist', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([2, 0, 1]),
                    lte: Buffer.from([3, 0, 1]),
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
        });

        describe('DatabaseReader', () => {
            it('should return false when called has if key does not exist', async () => {
                await expect(db.newReader().has(getRandomBytes())).resolves.toEqual(false);
            });

            it('should return true when called has if key exist', async () => {
                const kv = { key: getRandomBytes(), value: getRandomBytes() };
                const batch = new Batch();
                batch.set(kv.key, kv.value);
                await db.write(batch);

                await expect(db.newReader().has(kv.key)).resolves.toEqual(true);
            });

            it('should throw NotFoundError when data does not exist', async () => {
                await expect(db.newReader().get(getRandomBytes())).rejects.toThrow(NotFoundError);
            });

            it('should get the value if exist', async () => {
                const kv = { key: getRandomBytes(), value: getRandomBytes() };
                const batch = new Batch();
                batch.set(kv.key, kv.value);
                await db.write(batch);

                await expect(db.newReader().get(kv.key)).resolves.toEqual(kv.value);
            });

            describe('iteration', () => {
                let pairs;
                beforeAll(async () => {
                    await db.clear();
                    pairs = [
                        {
                            key: Buffer.from([0, 0, 0]),
                            value: getRandomBytes(),
                        },
                        {
                            key: Buffer.from([0, 0, 1]),
                            value: getRandomBytes(),
                        },
                        {
                            key: Buffer.from([1, 0, 0]),
                            value: getRandomBytes(),
                        },
                        {
                            key: Buffer.from([1, 0, 1]),
                            value: getRandomBytes(),
                        },
                    ];
                    const batch = new Batch();
                    for (const pair of pairs) {
                        batch.set(pair.key, pair.value);
                    }
                    await db.write(batch);
                });

                it('should iterate with specified range with limit', async () => {
                    const stream = db.newReader().iterate({
                        gte: Buffer.from([0, 0, 1]),
                        lte: Buffer.from([1, 0, 1]),
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

                    expect(values).toEqual(pairs.slice(1, 3));
                });

                it('should iterate with specified range with limit in reverse order', async () => {
                    const stream = db.newReader().iterate({
                        gte: Buffer.from([0, 0, 1]),
                        lte: Buffer.from([1, 0, 1]),
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

                    expect(values).toEqual(pairs.slice(2, 4).reverse());
                });

                it('should return empty if no data exist', async () => {
                    const stream = db.newReader().iterate({
                        gte: Buffer.from([2, 0, 1]),
                        lte: Buffer.from([3, 0, 1]),
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
            });
        });

        describe('checkpoint', () => {
            let tmpPath;
            beforeEach(() => {
                tmpPath = fs.mkdtempSync("");
            });
            afterEach(() => {
                fs.rmSync(tmpPath, { recursive: true, force: true });
            });

            it('should create checkpoint', async () => {
                const pairs = [
                    { key: getRandomBytes(), value: getRandomBytes() },
                    { key: getRandomBytes(), value: getRandomBytes() },
                ];
                const batch = new Batch();
                for (const kv of pairs) {
                    batch.set(kv.key, kv.value);
                }
                await db.write(batch);

                await expect(db.has(pairs[0].key)).resolves.toEqual(true);
                await expect(db.has(pairs[1].key)).resolves.toEqual(true);

                await db.checkpoint(tmpPath + '/test_db');

                db = new Database(tmpPath + '/test_db');
                await expect(db.has(pairs[0].key)).resolves.toEqual(true);
                await expect(db.has(pairs[1].key)).resolves.toEqual(true);
            });

            it('should failed to create checkpoint because directory is not empty', async () => {
                const pairs = [
                    { key: getRandomBytes(), value: getRandomBytes() },
                    { key: getRandomBytes(), value: getRandomBytes() },
                ];
                const batch = new Batch();
                for (const kv of pairs) {
                    batch.set(kv.key, kv.value);
                }
                await db.write(batch);

                await expect(db.has(pairs[0].key)).resolves.toEqual(true);
                await expect(db.has(pairs[1].key)).resolves.toEqual(true);

                await expect(db.checkpoint(tmpPath)).rejects.toThrow();
            });
        });
    });

    describe('InMemoryDatabase', () => {
        let db;

        beforeAll(() => {
            db = new InMemoryDatabase();
        });

        it('should open DB', () => {
            expect(db).not.toBeUndefined();
        });

        it('should return false when called has if key does not exist', async () => {
            await expect(db.has(getRandomBytes())).resolves.toEqual(false);
        });

        it('should return true when called has if key exist', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.has(kv.key)).resolves.toEqual(true);
        });

        it('should throw NotFoundError when data does not exist', async () => {
            await expect(db.get(getRandomBytes())).rejects.toThrow(NotFoundError);
        });

        it('should get the value if exist', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.get(kv.key)).resolves.toEqual(kv.value);
        });

        it('should clone the data', async () => {
            const kv = { key: getRandomBytes(), value: getRandomBytes() };
            const batch = new Batch();
            batch.set(kv.key, kv.value);
            await db.write(batch);

            await expect(db.get(kv.key)).resolves.toEqual(kv.value);

            const cloned = db.clone();
            // update original db to something else
            await db.set(kv.key, getRandomBytes());

            await expect(db.get(kv.key)).resolves.not.toEqual(kv.value);
            await expect(cloned.get(kv.key)).resolves.toEqual(kv.value);
        });

        describe('iteration', () => {
            let pairs;
            beforeAll(async () => {
                await db.clear();
                pairs = [
                    {
                        key: Buffer.from([0, 0, 0]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([0, 0, 1]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([1, 0, 0]),
                        value: getRandomBytes(),
                    },
                    {
                        key: Buffer.from([1, 0, 1]),
                        value: getRandomBytes(),
                    },
                ];
                const batch = new Batch();
                for (const pair of pairs) {
                    batch.set(pair.key, pair.value);
                }
                await db.write(batch);
            });

            it('should iterate with specified range with limit', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([0, 0, 1]),
                    lte: Buffer.from([1, 0, 1]),
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

                expect(values).toEqual(pairs.slice(1, 3));
            });

            it('should iterate with specified range with limit in reverse order', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([0, 0, 1]),
                    lte: Buffer.from([1, 0, 1]),
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

                expect(values).toEqual(pairs.slice(2, 4).reverse());
            });

            it('should return empty if no data exist', async () => {
                const stream = db.iterate({
                    gte: Buffer.from([2, 0, 1]),
                    lte: Buffer.from([3, 0, 1]),
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
        });
    });
});
