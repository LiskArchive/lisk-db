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

const { Readable } = require('stream');

class Iterator extends Readable {
    constructor(db, iterateFunc, options) {
        super();
        this._db = db;
        this._iterateFunc = iterateFunc;
        this._options = options;
        this.queue = []
        Readable.call(this, { objectMode: true });
        this._iterateFunc.call(
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

    _read() {
    }
}

module.exports = {
    Iterator,
};