const { Database, Batch, StateDB } = require("./");
const crypto = require('crypto');

const getRandomBytes = () => crypto.randomBytes(32);

(async () => {
    const db = new Database('.tmp', { readonly: false });
    console.log(db);
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
        console.log(await db.get(key));
    }

    console.log(Buffer.alloc(32, 5));
    const readable = db.iterate({ reverse: true, limit: 3, start: Buffer.alloc(32, 5) });
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
    for (let i = 0; i < 2; i++) {
        const key = getRandomBytes();
        const value = getRandomBytes();
        stateDB.set(key, value);
    }
    const root = await stateDB.commit(Buffer.from([]));
    console.log('root', root);

    await stateDB.close();


})()