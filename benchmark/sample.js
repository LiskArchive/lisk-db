const { Database, InMemoryDatabase, Batch, StateDB } = require("../main");
const crypto = require('crypto');

const getRandomBytes = () => crypto.randomBytes(32);

(async () => {
    const db = new Database('.tmp', { readonly: false });
    console.log('db', db);
    // const res = await db.get(Buffer.alloc(0));
    await db.set(Buffer.from('image', 'utf-8'), Buffer.from([1, 2, 3]));
    const existingVal = await db.get(Buffer.from('image', 'utf-8'));
    console.log({ existingVal });

    await db.del(Buffer.from('image', 'utf-8'));
    // await db.get(Buffer.from('image', 'utf-8'));

    const inserting = new Array(10).fill(0).map(() => ({ key: getRandomBytes(), value: getRandomBytes() }));
    const batch = new Batch();
    for (const { key, value } of inserting) {
        batch.set(key, value);
    }

    batch.del(getRandomBytes());

    await db.write(batch);

    for (const { key } of inserting) {
        console.log('get', await db.get(key));
    }

    console.log(Buffer.alloc(32, 5));
    const readable = db.iterate({ reverse: true, limit: 3, gte: Buffer.alloc(32, 5) });
    const list = await new Promise((resolve, reject) => {
        const result = [];
        readable
            .on('data', data => result.push(data))
            .on('error', err => reject(err))
            .on('end', () => resolve(result));
    });

    console.log(list);

    await db.close();



    const stateDB = new StateDB('.tmp-state', { readonly: false });
    stateDB.set(Buffer.from('0000000d00000000000000000000000000000000000000000001', 'hex'), Buffer.from('1', 'utf-8'));
    stateDB.set(Buffer.from('0000000d400000dfc343245e3382d92f4cfc328802ad184ac07a', 'hex'), Buffer.from('a', 'utf-8'));
    stateDB.set(Buffer.from('0000000d4000013b181a4ac911616e7115f5aa8133b0b38dc2eb', 'hex'), Buffer.from('b', 'utf-8'));

    const res = await stateDB.iterate({
        gte: Buffer.from('0000000d00000000000000000000000000000000000000000000', 'hex'),
        lte: Buffer.from('0000000d0000ffffffffffffffffffffffffffffffffffffffff', 'hex'),
        limit: -1,
        reverse: false,
    });
    console.log({ res });
    for (let i = 0; i < 2; i++) {
        const key = getRandomBytes();
        const value = getRandomBytes();
        stateDB.set(key, value);
    }
    const root = await stateDB.commit(Buffer.from([]));
    console.log('root', root);

    await stateDB.close();


})()