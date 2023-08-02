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

const { resolveObjectURL } = require("buffer");
const {
    state_db_new,
    state_db_close,
    state_db_get,
    state_db_get_current_state,
    state_db_exists,
    state_db_iterate,
    state_db_revert,
    state_db_commit,
    state_db_prove,
    state_db_verify,
    state_db_clean_diff_until,
    state_db_checkpoint,
    state_db_calculate_root,
    state_writer_new,
    state_writer_close,
    state_writer_snapshot,
    state_writer_restore_snapshot,
    state_db_reader_new,
    state_db_reader_close,
    state_db_reader_get,
    state_db_reader_exists,
    state_db_reader_iterate,
    state_db_read_writer_new,
    state_db_read_writer_close,
    state_db_read_writer_upsert_key,
    state_db_read_writer_get_key,
    state_db_read_writer_delete,
    state_db_read_writer_range,
} = require("./bin-package/index.node");

const { NotFoundError } = require('./error');
const { Iterator } = require("./iterator");
const { getOptionsWithDefault } = require('./options');
const { isInclusionProofForQueryKey } = require('./utils');

class StateReader {
    constructor(db) {
        this._db = state_db_reader_new(db);
    }

    close() {
        state_db_reader_close.call(this._db);
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            state_db_reader_get.call(this._db, key, (err, result) => {
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
            state_db_reader_exists.call(this._db, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    iterate(options = {}) {
        return new Iterator(this._db, state_db_reader_iterate, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new Iterator(this._db, state_db_reader_iterate, getOptionsWithDefault(options));
    }
}

class StateReadWriter {
    constructor(db) {
        this._db = state_db_read_writer_new(db);
        this._writer = state_writer_new();
    }

    get writer() {
        return this._writer;
    }

    close() {
        state_db_read_writer_close.call(this._db);
        state_writer_close.call(this.writer);
    }

    async get(key) {
        const value = await new Promise((resolve, reject) => {
            state_db_read_writer_get_key.call(this._db, this.writer, key, (err, result) => {
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
        return value;
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
        await new Promise((resolve, reject) => {
            state_db_read_writer_upsert_key.call(this._db, this.writer, key, value, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async del(key) {
        await new Promise((resolve, reject) => {
            state_db_read_writer_delete.call(this._db, this.writer, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async range(options = {}) {
        const defaultOptions = getOptionsWithDefault(options);
        const result = await new Promise((resolve, reject) => {
            state_db_read_writer_range.call(this._db, this.writer, defaultOptions, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
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

    async getCurrentState() {
        return new Promise((resolve, reject) => {
            state_db_get_current_state.call(this._db, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve({
                    root: result.root,
                    version: result.version,
                });
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
                // If result is empty, force to use different memory space from what's given from binding
                // Issue: https://github.com/nodejs/node/issues/32463
                for (const query of result.queries) {
                    if (query.value.length === 0) {
                        query.value = Buffer.alloc(0);
                    }
                }
                resolve(result);
            });
        });
    }

    async verify(root, queries, proof) {
        return new Promise((resolve, reject) => {
            state_db_verify.call(this._db, root, queries, proof, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }


    async verifyInclusionProof(root, queries, proof) {
        for (let i = 0; i < queries.length; i++) {
            if (!isInclusionProofForQueryKey(queries[i], proof.queries[i])) {
                return false;
            }
        }
        return this.verify(root, queries, proof);
    }

    async verifyNonInclusionProof(root, queries, proof) {
        for (let i = 0; i < queries.length; i++) {
            if (isInclusionProofForQueryKey(queries[i], proof.queries[i])) {
                return false;
            }
        }
        return this.verify(root, queries, proof);
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

    async calculateRoot(proof) {
        return new Promise((resolve, _reject) => {
            state_db_calculate_root.call(this._db, proof, (_err, result) => {
                resolve(result);
            });
        });
    }
}

module.exports = {
    StateDB,
    StateReadWriter,
    StateReader,
};
