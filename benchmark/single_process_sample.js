const { Database, Batch } = require('../index');
const bunyan = require('bunyan');

const crypto = require('crypto');

const getRandomBytes = () => crypto.randomBytes(32);

const logger = bunyan.createLogger({
    name: 'benchmark',
    streams: [
        {
            level: 'info',
            stream: process.stdout            // log INFO and above to stdout
        },
        {
            level: 'info',
            path: __dirname + '/benchmark.log'  // log ERROR and above to a file
        }
    ]
});


const db = new Database('./.tmp');

const currentMicros = () =>  {
    let hrTime = process.hrtime()
    return hrTime[0] * 1000000 + hrTime[1] / 1000;
}

const fetchNum = 10000;
const writeNum = fetchNum;

(async () => {
    let total = 0;
    for (let i = 0; i < 1000; i += 1) {
        const pairs = [];
        const batch = new Batch();
        for (let j = 0; j < writeNum; j += 1) {
            const pair = {
                key: getRandomBytes(100),
                value: getRandomBytes(5000),
            };
            total += 1;
            pairs.push(pair);
            batch.set(pair.key, pair.value);
        }

        const writeNow = currentMicros();
        await db.write(batch);
        logger.warn({ diff: currentMicros() - writeNow, total }, `write single ${writeNum}`);

        const fetchNow = currentMicros();
        for (const kv of pairs) {
            await db.get(kv.key);
        }
        logger.warn({ diff: currentMicros() - fetchNow, total }, `fetch single ${fetchNum}`);

    }
    db.close();
})()