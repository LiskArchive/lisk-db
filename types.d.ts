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

export interface StateDBOptions {
    readonly?: boolean;
    keyLength?: number;
}

export interface IterateOptions {
    limit?: number;
    reverse?: boolean;
    gte?: Buffer;
    lte?: Buffer;
}

export class NotFoundError extends Error { }

interface DatabaseReader {
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    iterate(options?: IterateOptions): NodeJS.ReadableStream;
    createReadStream(options?: IterateOptions): NodeJS.ReadableStream;
}

export class Database {
    constructor(path: string, option?: Options);
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    set(key: Buffer, value: Buffer): Promise<void>;
    del(key: Buffer): Promise<void>;
    write(batch: Batch): Promise<void>;
    iterate(options?: IterateOptions): NodeJS.ReadableStream;
    createReadStream(options?: IterateOptions): NodeJS.ReadableStream;
    clear(options?: IterateOptions): Promise<void>;
    close(): void;
    newReader(): DatabaseReader;
    checkpoint(path: string): Promise<void>;
}

export class InMemoryDatabase {
    constructor();
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    set(key: Buffer, value: Buffer): Promise<void>;
    del(key: Buffer): Promise<void>;
    write(batch: Batch): Promise<void>;
    iterate(options?: IterateOptions): NodeJS.ReadableStream;
    createReadStream(options?: IterateOptions): NodeJS.ReadableStream;
    clear(options?: IterateOptions): Promise<void>;
    clone(): InMemoryDatabase;
    close(): void;
}

export class Batch {
    set(key: Buffer, value: Buffer): void;
    del(key: Buffer): void;
}

declare class StateReader {
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    iterate(options?: IterateOptions): NodeJS.ReadableStream;
    createReadStream(options?: IterateOptions): NodeJS.ReadableStream;
    close(): void;
}

declare class StateReadWriter {
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    set(key: Buffer, value: Buffer): Promise<void>;
    del(key: Buffer): Promise<void>;
    range(options?: IterateOptions): Promise<{ key: Buffer, value: Buffer }[]>;
    snapshot(): number;
    restoreSnapshot(index: number): void;
    close(): void;
}

interface StateCommitOption {
    readonly?: boolean;
    checkRoot?: boolean;
    expectedRoot?: Buffer;
}

interface Proof {
    siblingHashes: Buffer[];
    queries: {
        key: Buffer;
        value: Buffer;
        bitmap: Buffer;
    }[];
}

interface CurrentState {
    root: Buffer;
    version: number;
}

export class StateDB {
    constructor(path: string, option?: StateDBOptions);
    get(key: Buffer): Promise<Buffer>;
    has(key: Buffer): Promise<boolean>;
    iterate(options?: IterateOptions): NodeJS.ReadableStream;
    createReadStream(options?: IterateOptions): NodeJS.ReadableStream;
    revert(prevRoot: Buffer, height: number): Promise<Buffer>;
    commit(readWriter: StateReadWriter, height: number, prevRoot: Buffer, options?: StateCommitOption): Promise<Buffer>;
    prove(root: Buffer, queries: Buffer[]): Promise<Proof>;
    verify(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    verifyInclusionProof(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    verifyNonInclusionProof(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    finalize(height: number): Promise<void>;
    newReader(): StateReader;
    newReadWriter(): StateReadWriter;
    close(): void;
    checkpoint(path: string): Promise<void>;
    getCurrentState(): Promise<CurrentState>;
    calculateRoot(proof: Proof): Promise<Buffer>;
}

export class SparseMerkleTree {
    constructor(keyLength?: number);
    update(root: Buffer, kvpair: { key: Buffer, value: Buffer }[]): Promise<Buffer>;
    prove(root: Buffer, queries: Buffer[]): Promise<Proof>;
    verify(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    verifyInclusionProof(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    verifyNonInclusionProof(root: Buffer, queries: Buffer[], proof: Proof): Promise<boolean>;
    calculateRoot(proof: Proof): Promise<Buffer>;
}
