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
    db_new,
    db_close,
    db_get,
    db_set,
    db_del,
    db_write,
    db_iterate,
    batch_new,
    batch_set,
    batch_del,
    state_db_new,
    state_db_close,
    state_db_get,
    state_db_set,
    state_db_del,
    state_db_clear,
    state_db_snapshot,
    state_db_snapshot_restore,
    state_db_iterate,
    state_db_revert,
    state_db_commit,
} = require("./index.node");
const { Readable } = require('stream');

class NotFoundError extends Error {
}

class Database {
    constructor(path, opts = {}) {
        this._db = db_new(path, opts);
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            db_get.call(this._db, key, (err, result) => {
                if (err) {
                    console.log(err.message)
                    if (err.message === 'No data') {
                        return reject(new NotFoundError('Data not found'));
                    }
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async set(key, value) {
        return new Promise((resolve, reject) => {
            db_set.call(this._db, key, value, err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    async del(key) {
        return new Promise((resolve, reject) => {
            db_del.call(this._db, key, err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    async write(batch) {
        return new Promise((resolve, reject) => {
            db_write.call(this._db, batch.inner, err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    iterate(options = {}) {
        const optionsWithDefault = {
            limit: -1,
            reverse: false,
            start: undefined,
            end: undefined,
            ...options,
        };
        return new Iterator(this._db, options);
    }

    close() {
        db_close.call(this._db);
    }
}

class Iterator extends Readable {
    constructor(db, options) {
        super();
        this._db = db;
        this._options = options;
        this.queue = []
        Readable.call(this, { objectMode: true });
    }

    _read() {
        db_iterate.call(
            this._db,
            this._options,
            (err, val) => {
                if (err) {
                    this.emit('error', err);
                    return;
                }
                this.push(val);
            },
            () => {
                this.push(null);
            },
        );
    }
}

class Batch {
    constructor() {
        this._batch = batch_new();
    }

    get inner() {
        return this._batch;
    }

    set(key, value) {
        batch_set.call(this._batch, key, value);
    }

    del(key) {
        batch_del.call(this._batch, key);
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
                        return reject(new NotFoundError('Data not found'));
                    }
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async iterate(options = {}) {
        const optionsWithDefault = {
            limit: -1,
            reverse: false,
            start: undefined,
            end: undefined,
            ...options,
        };
        return new Promise((resolve, reject) => {
            state_db_iterate.call(this._db, optionsWithDefault, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    async set(key, value) {
        state_db_set.call(this._db, key, value);
    }

    async del(key) {
        state_db_del.call(this._db, key);
    }

    clear() {
        state_db_clear.call(this._db);
    }

    snapshot() {
        state_db_snapshot.call(this._db);
    }

    restoreSnapshot() {
        state_db_snapshot_restore.call(this._db);
    }

    async revert(prev_root) {
        return new Promise((_, reject) => {
            reject(new Error('Not implemented'));
        });
    }

    async commit(prev_root) {
        return new Promise((resolve, reject) => {
            state_db_commit.call(this._db, prev_root, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    close() {
        state_db_close.call(this._db);
    }

}

module.exports = {
    Database,
    Batch,
    Iterator,
    StateDB,
};