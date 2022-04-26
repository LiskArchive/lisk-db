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

const getOptionsWithDefault = options => ({
    limit: options.limit !== undefined ? options.limit : -1,
    reverse: options.reverse !== undefined ? options.reverse : false,
    gte: options.gte !== undefined ? options.gte : undefined,
    lte: options.lte !== undefined ? options.lte : undefined,
});

module.exports = {
    getOptionsWithDefault,
};