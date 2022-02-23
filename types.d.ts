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

export interface Options {
    readonly?: boolean;
}

export interface IterateOptions {
    limit?: number;
    reverse?: boolean;
    start?: Buffer;
    end?: Buffer;
}

export class NotFoundError extends Error {}

export class Database {
    constructor(path: string, option?: Options);
    get(key: Buffer): Promise<Buffer>;
    set(key: Buffer, value: Buffer): Promise<void>;
    del(key: Buffer): Promise<void>;
    write(batch: Batch): Promise<void>;
    iterate(options?: IterateOptions): ReadableStream;
    close(): void;
}

export class Batch {
    set(key: Buffer, value: Buffer): void;
    del(key: Buffer): void;
}

export class StateDB {
    constructor(path: string, option?: Options);
    get(key: Buffer): Promise<Buffer>;
    set(key: Buffer, value: Buffer): void;
    del(key: Buffer): void;
    iterate(options?: IterateOptions): ReadableStream;
    clear(): void;
    snapshot(): void;
    restoreSnapshot(): void;
    commit(prevRoot: Buffer): Promise<Buffer>;
    close(): void;
}