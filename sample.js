const { Database, Batch } = require("./index");
const crypto = require('crypto');

const getRandomBytes = () => crypto.randomBytes(32);

(async () => {
    const db = new Database('.tmp', {readonly: false });
    console.log(db);
    // const res = await db.get(Buffer.alloc(0));
    await db.set(Buffer.from('image', 'utf-8'), Buffer.from([1,2,3]));
    const existingVal = await db.get(Buffer.from('image', 'utf-8'));
    console.log({ existingVal });

    await db.del(Buffer.from('image', 'utf-8'));
    // await db.get(Buffer.from('image', 'utf-8'));

    const inserting = new Array(10).fill(0).map(() => ({ key: getRandomBytes(), value: getRandomBytes()}));
    const batch = new Batch();
    for (const { key, value} of inserting) {
        batch.set(key, value);
    }

    batch.del(getRandomBytes());

    await db.write(batch);

    for (const {key} of inserting) {
        console.log(await db.get(key));
    }

    console.log(Buffer.alloc(32, 5));
    const readable = db.iterate({ reverse: true, limit: 3, start: Buffer.alloc(32, 5) });
    readable.on('data', data => console.log('data', data)).on('error', err => console.error(err)).on('end', () => console.log('end'));

    await new Promise(resolve => setTimeout(resolve, 1000));

    db.close();
})()