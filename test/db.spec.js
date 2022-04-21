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

const os = require('os');
const path = require('path');
const fs = require('fs');
const { Database } = require('../');

describe.only('Database', () => {
    describe('constructor', () => {
        it('should open DB', () => {
            const dbPath = path.join(os.tmpdir(), Date.now().toString());
            fs.mkdirSync(dbPath, { recursive: true });
            const db = new Database(dbPath);
            expect(db).not.toBeUndefined();
            db.close();
        });
    });
});