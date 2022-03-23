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

process.on('message', msg => {
    if (msg.type === 'get') {
        db.get(Buffer.from(msg.key, 'hex'))
            .then(val => {
                process.send({
                    id: msg.id,
                    type: 'get_resp',
                    value: val.toString('hex'),
                });
            })
            .catch(err => {
                process.send({
                    id: msg.id,
                    type: 'get_resp',
                    err,
                });
            });
        return;
    }

    if (msg.type === 'set') {
        const batch = new Batch();
        for (const kv of msg.data) {
            batch.set(Buffer.from(kv.key, 'hex'), Buffer.from(kv.value, 'hex'));
        }
        db.write(batch)
            .then(val => {
                process.send({
                    id: msg.id,
                    type: 'set_resp',
                    err: undefined,
                });
            })
            .catch(err => {
                process.send({
                    id: msg.id,
                    type: 'set_resp',
                    err,
                });
            });
    }
});

// (async () => {
//     for (let i = 0; i < 10000000; i += 1) {
//         const pair = {
//             key: getRandomBytes(100),
//             value: getRandomBytes(5000),
//         };
//         await db.set(pair.key, pair.value);
//         process.send({
//             type: 'written',
//             pair: {
//                 key: pair.key.toString('hex'),
//                 value: pair.value.toString('hex'),
//             },
//         });
//         logger.debug(pair.key.toString('hex'), 'written');
//         // await new Promise(resolve => setTimeout(resolve, 500));
//     }
// })()