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
    in_memory_smt_new,
    in_memory_smt_update,
    in_memory_smt_prove,
} = require("./bin-package/index.node");

const DEFAULT_KEY_LENGTH = 38;
class SparseMerkleTree {
    constructor(keyLength = DEFAULT_KEY_LENGTH) {
        this._inner = in_memory_smt_new(keyLength);
    }

    async update(root, kvpairs) {
        return new Promise((resolve, reject) => {
            in_memory_smt_update.call(this._inner, root, kvpairs, (err, result) => {
                if (err) {
                    reject(err);
                    return;
                }
                resolve(result);
            });
        });
    }

    async prove(root, queries) {
        return new Promise((resolve, reject) => {
            in_memory_smt_prove.call(this._inner, root, queries, (err, result) => {
                if (err) {
                    reject(err);
                    return;
                }
                resolve(result);
            });
        });
    }
}

module.exports = {
    SparseMerkleTree,
};