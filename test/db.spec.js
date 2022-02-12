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

const { Database } = require('../index');

describe('Database', () => {
    describe('constructor', () => {
        it('should open DB', () => {
            const db = new Database('./.tmp', { readonly: true });
            expect(db).not.toBeUndefined();
            db.close();
        });
    });
});