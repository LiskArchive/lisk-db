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
    db_clear,
    db_close,
    db_get,
    db_exists,
    db_set,
    db_del,
    db_write,
    db_iterate,
    batch_new,
    batch_set,
    batch_del,
    in_memory_db_new,
    in_memory_db_get,
    in_memory_db_set,
    in_memory_db_del,
    in_memory_db_clear,
    in_memory_db_write,
    in_memory_db_iterate,
} = require("./index.node");
const { Readable } = require('stream');
const { NotFoundError } = require('./error');
const { Iterator } = require('./iterator');
const { getOptionsWithDefault } = require('./options');

class Reader {
    constructor(db) {
        this._db = db;
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            db_get.call(this._db, key, (err, result) => {
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

    async has(key) {
        return new Promise((resolve, reject) => {
            db_exists.call(this._db, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    iterate(options = {}) {
        return new Iterator(this._db, db_iterate, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new Iterator(this._db, db_iterate, getOptionsWithDefault(options));
    }
}

class Database {
    constructor(path, opts = {}) {
        this._db = db_new(path, opts);
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            db_get.call(this._db, key, (err, result) => {
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

    async has(key) {
        return new Promise((resolve, reject) => {
            db_exists.call(this._db, key, (err, result) => {
                if (err) {
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
        return new Iterator(this._db, db_iterate, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new Iterator(this._db, db_iterate, getOptionsWithDefault(options));
    }

    async clear(options = {}) {
        return new Promise((resolve, reject) => {
            db_clear.call(this._db, getOptionsWithDefault(options), err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    newReader() {
        return new Reader(this._db);
    }

    close() {
        db_close.call(this._db);
    }
}

class InMemoryIterator extends Readable {
    constructor(db, options) {
        super();
        this._db = db;
        this._options = options;
        Readable.call(this, { objectMode: true });
    }

    _read() {
        in_memory_db_iterate.call(
            this._db,
            this._options,
            (err, results) => {
                if (err) {
                    this.emit('error', err);
                    return;
                }
                for (const result of results) {
                    this.push(result);
                }
                this.push(null)
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

    del(key) { batch_del.call(this._batch, key);
    }
}



class InMemoryDatabase {
    constructor() {
        this._db = in_memory_db_new();
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            in_memory_db_get.call(this._db, key, (err, result) => {
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

    async has(key) {
        return new Promise((resolve, reject) => {
            in_memory_db_get.call(this._db, key, (err, result) => {
                if (err) {
                    if (err.message === 'No data') {
                        return reject(false);
                    }
                    return reject(err);
                }
                resolve(true);
            });
        });
    }

    async set(key, value) {
        return new Promise((resolve, reject) => {
            in_memory_db_set.call(this._db, key, value);
            resolve();
        });
    }

    async del(key) {
        return new Promise((resolve, reject) => {
            in_memory_db_del.call(this._db, key);
            resolve();
        });
    }

    async write(batch) {
        return new Promise((resolve, reject) => {
            in_memory_db_write.call(this._db, batch.inner, err => {
                if (err) {
                    return reject(err);
                }
                resolve();
            });
        });
    }

    iterate(options = {}) {
        return new InMemoryIterator(this._db, getOptionsWithDefault(options));
    }

    createReadStream(options = {}) {
        return new InMemoryIterator(this._db, getOptionsWithDefault(options));
    }

    async clear(options = {}) {
        in_memory_db_clear.call(this._db);
    }

    close() {
        in_memory_db_clear.call(this._db);
    }
}

module.exports = {
    Database,
    InMemoryDatabase,
    Batch,
};