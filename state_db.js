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

const {
    state_db_new,
    state_db_close,
    state_db_get,
    state_db_exists,
    state_db_iterate,
    state_db_revert,
    state_db_commit,
    state_db_prove,
    state_db_verify,
    state_db_clean_diff_until,
    state_db_checkpoint,
    state_writer_new,
    state_writer_get,
    state_writer_update,
    state_writer_del,
    state_writer_is_cached,
    state_writer_get_range,
    state_writer_cache_new,
    state_writer_cache_existing,
    state_writer_snapshot,
    state_writer_restore_snapshot,
} = require("./bin-package/index.node");

const { NotFoundError } = require('./error');
const { Iterator } = require("./iterator");
const { getOptionsWithDefault } = require('./options');


class StateReader {
    constructor(db) {
        this._db = db;
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            state_db_get.call(this._db, key, (err, result) => {
                if (err) {
                    if (err.message === 'No data') {
                        return reject(new NotFoundError(`Key ${key.toString('hex')} does not exist.`));
                    }
                    return reject(err);
                }
                // If result is empty, force to use different memory space from what's given from binding
                // Issue: https://github.com/nodejs/node/issues/32463
                if (result.length === 0) {
                    resolve(Buffer.alloc(0));
                    return;
                }
                resolve(result);
            });
        });
    }

    async has(key) {
        return new Promise((resolve, reject) => {
            state_db_exists.call(this._db, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    iterate(options = {}) {
        return new Iterator(this._db, state_db_iterate, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new Iterator(this._db, state_db_iterate, getOptionsWithDefault(options));
    }
}

class StateReadWriter {
    constructor(db) {
        this._db = db;
        this._writer = state_writer_new();
    }

    get writer() {
        return this._writer;
    }

    async get(key) {
        const { value, deleted, exists } = state_writer_get.call(this._writer, key);
        if (exists && !deleted) {
            return value;
        }
        if (deleted) {
            throw new NotFoundError(`Key ${key.toString('hex')} does not exist.`);
        }

        const fetched = await new Promise((resolve, reject) => {
            state_db_get.call(this._db, key, (err, result) => {
                if (err) {
                    if (err.message === 'No data') {
                        return reject(new NotFoundError(`Key ${key.toString('hex')} does not exist.`));
                    }
                    return reject(err);
                }
                // If result is empty, force to use different memory space from what's given from binding
                // Issue: https://github.com/nodejs/node/issues/32463
                if (result.length === 0) {
                    resolve(Buffer.alloc(0));
                    return;
                }
                resolve(result);
            });
        });
        state_writer_cache_existing.call(this._writer, key, fetched);
        return fetched;
    }

    async has(key) {
        try {
            await this.get(key);
            return true;
        } catch (error) {
            if (!(error instanceof NotFoundError)) {
                throw error;
            }
            return false;
        }
    }

    async set(key, value) {
        const cached = state_writer_is_cached.call(this._writer, key);
        if (cached) {
            state_writer_update.call(this._writer, key, value);
            return;
        }
        const dataExist = await this._ensureCache(key);
        if (dataExist) {
            state_writer_update.call(this._writer, key, value);
            return;
        }
        state_writer_cache_new.call(this._writer, key, value);
    }

    async del(key) {
        const cached = state_writer_is_cached.call(this._writer, key);
        if (!cached) {
            await this._ensureCache(key);
        }
        state_writer_del.call(this._writer, key);
    }

    async range(options = {}) {
        const defaultOptions = getOptionsWithDefault(options);
        const stream = new Iterator(this._db, state_db_iterate, defaultOptions);
        const storedData = await new Promise((resolve, reject) => {
            const values = [];
            stream
                .on('data', ({ key, value }) => {
                    const { value: cachedValue, deleted, exists } = state_writer_get.call(this._writer, key);
                    // if key is already stored in cache, return cached value
                    if (exists && !deleted) {
                        values.push({
                            key,
                            value: cachedValue,
                        });
                        return;
                    }
                    // if deleted in cache, do not include
                    if (deleted) {
                        return;
                    }
                    state_writer_cache_existing.call(this._writer, key, value);
                    values.push({
                        key,
                        value,
                    });
                })
                .on('error', error => {
                    reject(error);
                })
                .on('end', () => {
                    resolve(values);
                });
        });
        const cachedValues = state_writer_get_range.call(this._writer, defaultOptions.gte, defaultOptions.lte);
        const existingKey = {};
        const result = [];
        for (const data of cachedValues) {
            existingKey[data.key.toString('binary')] = true;
            result.push({
                key: data.key,
                value: data.value,
            });
        }
        for (const data of storedData) {
            if (existingKey[data.key.toString('binary')] === undefined) {
                result.push({
                    key: data.key,
                    value: data.value,
                });
            }
        }
        result.sort((a, b) => {
            if (options.reverse) {
                return b.key.compare(a.key);
            }
            return a.key.compare(b.key);
        });
        if (options.limit) {
            result.splice(options.limit);
        }
        return result;
    }

    snapshot() {
        let result = state_writer_snapshot.call(this._writer);
        return result;
    }

    restoreSnapshot(index = 0) {
        state_writer_restore_snapshot.call(this._writer, index);
    }

    async _ensureCache(key) {
        try {
            const value = await new Promise((resolve, reject) => {
                state_db_get.call(this._db, key, (err, result) => {
                    if (err) {
                        if (err.message === 'No data') {
                            return reject(new NotFoundError(`Key ${key.toString('hex')} does not exist.`));
                        }
                        return reject(err);
                    }
                    // If result is empty, force to use different memory space from what's given from binding
                    // Issue: https://github.com/nodejs/node/issues/32463
                    if (result.length === 0) {
                        resolve(Buffer.alloc(0));
                        return;
                    }
                    resolve(result);
                });
            });
            state_writer_cache_existing.call(this._writer, key, value);
            return true;
        } catch (error) {
            if (error instanceof NotFoundError) {
                return false;
            }
            throw error;
        }
    }
}

class StateDB {
    constructor(path, opts = {}) {
        this._db = state_db_new(path, opts);
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            state_db_get.call(this._db, key, (err, result) => {
                if (err) {
                    if (err.message === 'No data') {
                        return reject(new NotFoundError(`Key ${key.toString('hex')} does not exist.`));
                    }
                    return reject(err);
                }
                // If result is empty, force to use different memory space from what's given from binding
                // Issue: https://github.com/nodejs/node/issues/32463
                if (result.length === 0) {
                    resolve(Buffer.alloc(0));
                    return;
                }
                resolve(result);
            });
        });
    }

    async has(key) {
        return new Promise((resolve, reject) => {
            state_db_exists.call(this._db, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    iterate(options = {}) {
        return new Iterator(this._db, state_db_iterate, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new Iterator(this._db, state_db_iterate, getOptionsWithDefault(options));
    }

    async revert(prev_root, height) {
        return new Promise((resolve, reject) => {
            state_db_revert.call(this._db, prev_root, height, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async commit(readWriter, height, prevRoot, options = {}) {
        const defaultOptions = {
            readonly: options.readonly !== undefined ? options.readonly : false,
            checkRoot: options.checkRoot !== undefined ? options.checkRoot : false,
            expectedRoot: options.expectedRoot !== undefined ? options.expectedRoot : Buffer.alloc(0),
        };
        return new Promise((resolve, reject) => {
            state_db_commit.call(this._db, readWriter.writer, height, prevRoot, defaultOptions.readonly, defaultOptions.expectedRoot, defaultOptions.checkRoot, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async prove(root, queries) {
        return new Promise((resolve, reject) => {
            state_db_prove.call(this._db, root, queries, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async prove(root, queries, proof) {
        return new Promise((resolve, reject) => {
            state_db_verify.call(this._db, root, queries, proof, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async finalize(height) {
        return new Promise((resolve, reject) => {
            state_db_clean_diff_until.call(this._db, height, (err) => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    newReader() {
        return new StateReader(this._db);
    }

    newReadWriter() {
        return new StateReadWriter(this._db);
    }

    close() {
        state_db_close.call(this._db);
    }

    async checkpoint(path) {
        return new Promise((resolve, reject) => {
            state_db_checkpoint.call(this._db, path, err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }
}

module.exports = {
    StateDB,
    StateReadWriter,
    StateReader,
};