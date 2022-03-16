const { Database } = require('../index');
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

(async () => {
    const db = new Database('./.tmp');
    for (let i = 0; i < 10000000; i += 1) {
        const pair = {
            key: getRandomBytes(100),
            value: getRandomBytes(5000),
        };
        await db.set(pair.key, pair.value);
        process.send({
            type: 'written',
            pair: {
                key: pair.key.toString('hex'),
                value: pair.value.toString('hex'),
            },
        });
        logger.info(pair.key.toString('hex'), 'written');
        // await new Promise(resolve => setTimeout(resolve, 500));
    }
})()