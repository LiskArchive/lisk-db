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
    in_memory_smt_verify,
    in_memory_smt_calculate_root,
} = require("./bin-package/index.node");
const { isInclusionProofForQueryKey } = require('./utils');

const DEFAULT_KEY_LENGTH = 38;

const copyBuffer = h => {
    const copied = Buffer.alloc(h.length);
    h.copy(copied);
    return copied;
}
class SparseMerkleTree {
    constructor(keyLength = DEFAULT_KEY_LENGTH) {
        this._keyLength = keyLength;
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
                // If result is empty, force to use different memory space from what's given from binding
                // Issue: https://github.com/nodejs/node/issues/32463
                // Try console.log(proof) at the end of Jumbo fixture test without copy
                resolve({
                    siblingHashes: result.siblingHashes.map(copyBuffer),
                    queries: result.queries.map(q => ({
                        key: copyBuffer(q.key),
                        value: copyBuffer(q.value),
                        bitmap: copyBuffer(q.bitmap),
                    })),
                });
            });
        });
    }

    async verify(root, queries, proof) {
        return new Promise((resolve, reject) => {
            in_memory_smt_verify.call(null, root, queries, proof, this._keyLength, (err, result) => {
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

    async calculateRoot(proof) {
        return new Promise((resolve, reject) => {
            in_memory_smt_calculate_root.call(null, proof, (err, result) => {
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
