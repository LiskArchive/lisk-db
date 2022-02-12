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

const { db_new, db_close, db_get } = require("./index.node");

class Database {
    constructor(path, opts = {}) {
        this._db = db_new(path, opts);
    }

    async get(key) {
        return new Promise((resolve, reject) => {
            db_get.call(this._db, key, (err, result) => {
                if (err) {
                    return reject(err);
                }
                resolve(result);
            });
        });
    }

    close() {
        db_close.call(this._db);
    }
}

module.exports = {
    Database,
};